//! Popup de mídia para o clique no ícone da Waybar: o porte de `home/.local/bin/waybar-media-popup`, que
//! passa a vez a este binário (sway_rust). Capa, faixa, progresso e os controles, pelo MPRIS. Com mais de
//! um player com mídia, uma linha de botões troca entre eles. Clicar de novo no ícone, Esc, clicar fora ou
//! trocar de workspace fecha.
//!
//! As leituras do barramento rodam em outra linha (um player travado não pode travar a janela). A altura
//! segue a da capa (a de um disco é quadrada, a de um vídeo é larga): cada mudança abre a carência em que
//! o focus-out do Sway não fecha (rendered_at).
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, gio, glib, pango};
use swaydesk_common::i18n::tr;
use swaydesk_media::{self as wm, art, State};
use swaydesk_ui::list::{CSS_OPT, CSS_SCALE};
use swaydesk_ui::{label, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-media-popup";
// A capa cabe em COVER_WIDTH × COVER_HEIGHT com a proporção da imagem
const COVER_WIDTH: f64 = 320.0;
const COVER_HEIGHT: i32 = 200;
/// Uma capa pequena cresce até aqui: mais que isso, fica granulada
const MAX_ZOOM: f64 = 1.5;
const CARD_WIDTH: i32 = 360;
/// Depois de arrastar, a posição lida espera o player chegar lá
const SEEK_HOLD: Duration = Duration::from_millis(1500);
/// Arrastar manda só a última posição
const SEEK_DELAY: Duration = Duration::from_millis(150);

// Material da Nerd Font
const ICON_MUSIC: &str = "\u{f075a}";
const ICON_PREVIOUS: &str = "\u{f04ae}";
const ICON_NEXT: &str = "\u{f04ad}";
const ICON_PLAY: &str = "\u{f040a}";
const ICON_PAUSE: &str = "\u{f03e4}";

const CSS: &str = r#"
.cover { background: #38383d; border-radius: 8px; }
.cover-glyph { font-family: "Symbols Nerd Font"; font-size: 72px; color: #8a8a8f; }
.media-title { font-size: 15px; font-weight: bold; }
.media-artist { font-size: 13px; }
.media-album, .media-time { color: #8a8a8f; font-size: 12px; }
.media-empty { color: #8a8a8f; font-size: 13px; }
button.ctl {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  font-family: "Symbols Nerd Font"; font-size: 20px; min-height: 0; min-width: 0;
  padding: 4px 10px; border-radius: 18px;
}
button.ctl:hover { background: #4c4c50; }
button.ctl:disabled { color: #5a5a5f; }
button.ctl.main { background: @accent; color: @on_accent; font-size: 22px; padding: 6px 12px; }
button.ctl.main:hover { background: @accent_hover; }
button.ctl.main:disabled { background: #38383d; color: #5a5a5f; }
"#;

/// O tamanho da capa: cabe na área, com a proporção da imagem, ampliada no máximo MAX_ZOOM vezes.
fn fit(width: i32, height: i32) -> (i32, i32) {
    let factor = MAX_ZOOM.min(COVER_WIDTH / f64::from(width.max(1))).min(f64::from(COVER_HEIGHT) / f64::from(height.max(1)));
    (((f64::from(width) * factor).round() as i32).max(1), ((f64::from(height) * factor).round() as i32).max(1))
}

/// A capa no tamanho do fit, só pelo carregador do formato que os bytes mostram.
fn cover_pixbuf(data: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    let kind = art::image_type(data)?;
    let loader = gdk_pixbuf::PixbufLoader::with_type(kind).ok()?;
    loader.connect_size_prepared(|loader, w, h| {
        let (w, h) = fit(w, h);
        loader.set_size(w, h);
    });
    let written = loader.write(data);
    let closed = loader.close();
    written.and(closed).ok()?;
    loader.pixbuf()
}

/// O resultado de uma leitura: tudo, ou só alguns nomes (None: o player não respondeu).
enum Read {
    All(Vec<State>),
    Some(Vec<(String, Option<State>)>),
}

struct Popup {
    popup: Rc<PopupWindow>,
    bus: gio::DBusConnection,
    players: gtk4::Box,
    stack: gtk4::Stack,
    cover: gtk4::Stack,
    picture: gtk4::Picture,
    title: gtk4::Label,
    artist: gtk4::Label,
    album: gtk4::Label,
    scale: gtk4::Scale,
    elapsed: gtk4::Label,
    total: gtk4::Label,
    previous: gtk4::Button,
    play: gtk4::Button,
    next: gtk4::Button,
    /// nome → State de cada player que respondeu
    known: RefCell<BTreeMap<String, State>>,
    /// nome → Identity: não muda enquanto o dono for o mesmo
    identities: RefCell<HashMap<String, String>>,
    /// nome → nome único (":1.42"), de quem vêm os avisos
    owners: RefCell<HashMap<String, String>>,
    /// Os que têm mídia, na ordem dos nomes
    states: RefCell<Vec<State>>,
    current: RefCell<Option<State>>,
    preferred: RefCell<Option<String>>,
    reading: Cell<bool>,
    want_all: Cell<bool>,
    want: RefCell<BTreeSet<String>>,
    seek_hold: Cell<Option<Instant>>,
    seek_timer: RefCell<Option<glib::SourceId>>,
    seek_target: RefCell<Option<(State, i64)>>,
    art_url: RefCell<Option<String>>,
    /// (posição lida, quando): o progresso anda entre as leituras
    position_at: Cell<(i64, Instant)>,
    cover_size: Cell<Option<(i32, i32)>>,
    _subscriptions: RefCell<Vec<gio::SignalSubscriptionId>>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Option<Rc<Popup>> {
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
        let popup = PopupWindow::new(PROGRAM, "waybar-media", &format!("{CSS_OPT}{CSS_SCALE}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        card.add_css_class("card");
        card.set_size_request(CARD_WIDTH, -1);
        popup.window.set_child(Some(&card));

        // Uma linha por player com mídia; só aparece com mais de um
        let players = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        players.set_homogeneous(true);
        players.set_visible(false);
        card.append(&players);

        // O conteúdo e o "nada tocando" no mesmo lugar, com o tamanho do maior
        let stack = gtk4::Stack::new();
        stack.set_hhomogeneous(true);
        stack.set_vhomogeneous(true);
        card.append(&stack);

        let media = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        // A capa no alto, centralizada; sem altura fixa: a página visível dá o tamanho
        let cover = gtk4::Stack::new();
        cover.set_vhomogeneous(false);
        cover.set_margin_bottom(4);
        let picture = gtk4::Picture::new();
        picture.set_can_shrink(false);
        picture.set_halign(gtk4::Align::Center);
        picture.set_valign(gtk4::Align::Center);
        cover.add_named(&picture, Some("image"));
        let glyph = label(ICON_MUSIC, &["cover-glyph", "cover"], 0.5);
        glyph.set_size_request(COVER_HEIGHT, COVER_HEIGHT);
        glyph.set_halign(gtk4::Align::Center);
        cover.add_named(&glyph, Some("glyph"));
        media.append(&cover);
        let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let (title, artist, album) = (label("", &["media-title"], 0.5), label("", &["media-artist"], 0.5), label("", &["media-album"], 0.5));
        for l in [&title, &artist, &album] {
            l.set_ellipsize(pango::EllipsizeMode::End);
            l.set_max_width_chars(1); // o rótulo segue a largura do cartão, e não a do texto
            text.append(l);
        }
        media.append(&text);
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.0, 1.0);
        scale.set_draw_value(false);
        media.append(&scale);
        let times = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let (elapsed, total) = (label("0:00", &["media-time"], 0.0), label("0:00", &["media-time"], 1.0));
        elapsed.set_hexpand(true);
        times.append(&elapsed);
        times.append(&total);
        media.append(&times);
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        controls.set_halign(gtk4::Align::Center);
        let control = |glyph: &str, tip: &str| {
            let b = gtk4::Button::with_label(glyph);
            b.add_css_class("ctl");
            b.set_tooltip_text(Some(tip));
            b.set_focusable(false);
            controls.append(&b);
            b
        };
        let previous = control(ICON_PREVIOUS, &tr("Previous"));
        let play = control(ICON_PLAY, &tr("Play"));
        play.add_css_class("main");
        let next = control(ICON_NEXT, &tr("Next"));
        media.append(&controls);
        stack.add_named(&media, Some("media"));
        let empty = label(&tr("Nothing playing"), &["media-empty"], 0.5);
        empty.set_valign(gtk4::Align::Center);
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&gtk4::Box::new(gtk4::Orientation::Vertical, 0), Some("loading")); // até a primeira leitura
        stack.set_visible_child_name("loading");
        cover.set_visible_child_name("glyph"); // até a primeira capa: o popup abre no tamanho de um disco

        let this = Rc::new(Popup {
            popup,
            bus,
            players,
            stack,
            cover,
            picture,
            title,
            artist,
            album,
            scale,
            elapsed,
            total,
            previous,
            play,
            next,
            known: RefCell::default(),
            identities: RefCell::default(),
            owners: RefCell::default(),
            states: RefCell::default(),
            current: RefCell::default(),
            preferred: RefCell::default(),
            reading: Cell::new(false),
            want_all: Cell::new(true),
            want: RefCell::default(),
            seek_hold: Cell::new(None),
            seek_timer: RefCell::default(),
            seek_target: RefCell::default(),
            art_url: RefCell::default(),
            position_at: Cell::new((0, Instant::now())),
            cover_size: Cell::new(None),
            _subscriptions: RefCell::default(),
        });
        for (button, method) in [(&this.previous, "Previous"), (&this.play, "PlayPause"), (&this.next, "Next")] {
            let me = Rc::downgrade(&this);
            button.connect_clicked(move |_| {
                if let Some(p) = me.upgrade() {
                    p.send(method);
                }
            });
        }
        let me = Rc::downgrade(&this);
        this.scale.connect_change_value(move |_, _, value| {
            if let Some(p) = me.upgrade() {
                p.on_seek(value);
            }
            glib::Propagation::Stop
        });
        this.subscribe();
        // A janela aparece já, com o tamanho final: esperar por um player travado deixaria a tela sem
        // clique e sem popup
        this.popup.show();
        this.refresh(None);
        let me = Rc::downgrade(&this);
        glib::timeout_add_local(Duration::from_millis(500), move || match me.upgrade() {
            Some(p) => {
                p.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        // Um aviso perdido não deixa a janela parada
        let me = Rc::downgrade(&this);
        glib::timeout_add_seconds_local(5, move || match me.upgrade() {
            Some(p) => {
                p.refresh(None);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        Some(this)
    }

    #[allow(deprecated)] // o signal_subscribe com closure continua o caminho simples no gio 0.20
    fn subscribe(self: &Rc<Self>) {
        let mut subs = Vec::new();
        for (iface, member) in [("org.freedesktop.DBus.Properties", "PropertiesChanged"), (wm::PLAYER, "Seeked")] {
            let me = Rc::downgrade(self);
            subs.push(self.bus.signal_subscribe(None, Some(iface), Some(member), Some(wm::PATH), None, gio::DBusSignalFlags::NONE, move |_, sender, _, _, _, _| {
                // Um aviso de um player relê só ele
                if let Some(p) = me.upgrade() {
                    let name = p.owners.borrow().iter().find(|(_, o)| o.as_str() == sender).map(|(n, _)| n.clone());
                    p.refresh(name);
                }
            }));
        }
        let me = Rc::downgrade(self);
        subs.push(self.bus.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(wm::MPRIS),
            gio::DBusSignalFlags::MATCH_ARG0_NAMESPACE,
            move |_, _, _, _, _, params| {
                let Some(p) = me.upgrade() else { return };
                // Outro processo com o mesmo nome: o Identity e o dono antigos não valem mais
                if let Some((name, _old, new)) = params.get::<(String, String, String)>() {
                    p.identities.borrow_mut().remove(&name);
                    p.owners.borrow_mut().remove(&name);
                    if !new.is_empty() {
                        p.owners.borrow_mut().insert(name, new);
                    }
                }
                p.refresh(None);
            },
        ));
        self._subscriptions.replace(subs);
    }

    // --- dados ---
    fn refresh(self: &Rc<Self>, name: Option<String>) {
        match name {
            None => self.want_all.set(true),
            Some(name) => {
                self.want.borrow_mut().insert(name);
            }
        }
        self.read_pending();
    }

    fn read_pending(self: &Rc<Self>) {
        if self.reading.get() {
            return; // a leitura em andamento termina e chama de novo
        }
        let everything = self.want_all.replace(false);
        let names: Vec<String> = std::mem::take(&mut *self.want.borrow_mut()).into_iter().collect();
        if !everything && names.is_empty() {
            return;
        }
        self.reading.set(true);
        let (identities, owners, bus) = (self.identities.borrow().clone(), self.owners.borrow().clone(), self.bus.clone());
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let known = |n: &str| (identities.get(n).cloned(), owners.get(n).cloned());
                if everything {
                    Read::All(wm::read_all(&bus, &known))
                } else {
                    Read::Some(
                        names
                            .iter()
                            .map(|n| {
                                let (i, o) = known(n);
                                (n.clone(), wm::read_state(&bus, n, i.as_deref(), o.as_deref()))
                            })
                            .collect(),
                    )
                }
            })
            .await;
            let Some(p) = me.upgrade() else { return };
            p.reading.set(false);
            // Sempre volta: o que ficou pendente é lido mesmo se a leitura falhou
            if let Ok(result) = result {
                p.apply(result);
            }
            p.read_pending();
        });
    }

    fn apply(self: &Rc<Self>, result: Read) {
        {
            let mut known = self.known.borrow_mut();
            match result {
                Read::All(states) => *known = states.into_iter().map(|s| (s.name.clone(), s)).collect(),
                Read::Some(pairs) => {
                    for (name, state) in pairs {
                        match state {
                            Some(s) => {
                                known.insert(name, s);
                            }
                            None => {
                                known.remove(&name);
                            }
                        }
                    }
                }
            }
            for st in known.values() {
                if !st.identity.is_empty() {
                    // Um Identity vazio é de antes de o player terminar de abrir: pergunta de novo
                    self.identities.borrow_mut().insert(st.name.clone(), st.identity.clone());
                }
                if !st.owner.is_empty() {
                    self.owners.borrow_mut().entry(st.name.clone()).or_insert_with(|| st.owner.clone());
                }
            }
            *self.states.borrow_mut() = known.values().filter(|s| s.active()).cloned().collect();
        }
        let current = wm::pick(&self.states.borrow(), self.preferred.borrow().as_deref()).cloned();
        self.current.replace(current);
        self.show_players();
        self.show_current();
    }

    fn show_players(self: &Rc<Self>) {
        let states = self.states.borrow().clone();
        let labels = wm::player_labels(&states);
        let shown: Vec<(String, String)> = std::iter::successors(self.players.first_child(), |c| c.next_sibling())
            .map(|c| (c.widget_name().to_string(), c.tooltip_text().map(|t| t.to_string()).unwrap_or_default()))
            .collect();
        let wanted: Vec<(String, String)> = states.iter().map(|s| s.name.clone()).zip(labels.iter().cloned()).collect();
        if shown != wanted {
            self.resized(); // a linha pode aparecer ou sumir
            while let Some(child) = self.players.first_child() {
                self.players.remove(&child);
            }
            for (name, text) in &wanted {
                let button = gtk4::Button::new();
                let l = gtk4::Label::new(Some(text));
                l.set_ellipsize(pango::EllipsizeMode::End);
                l.set_max_width_chars(1); // o botão segue a largura da linha, e não a do nome
                button.set_child(Some(&l));
                button.set_tooltip_text(Some(text));
                button.set_widget_name(name);
                button.add_css_class("opt");
                let (me, name) = (Rc::downgrade(self), name.clone());
                button.connect_clicked(move |_| {
                    if let Some(p) = me.upgrade() {
                        p.choose(&name);
                    }
                });
                self.players.append(&button);
            }
            self.players.set_visible(states.len() > 1);
        }
        let current = self.current.borrow().as_ref().map(|s| s.name.clone());
        for child in std::iter::successors(self.players.first_child(), |c| c.next_sibling()) {
            if Some(child.widget_name().to_string()) == current {
                child.add_css_class("active");
            } else {
                child.remove_css_class("active");
            }
        }
    }

    fn choose(self: &Rc<Self>, name: &str) {
        self.preferred.replace(Some(name.to_string()));
        let current = wm::pick(&self.states.borrow(), Some(name)).cloned();
        self.current.replace(current);
        self.show_players();
        self.show_current();
    }

    fn show_current(self: &Rc<Self>) {
        let Some(state) = self.current.borrow().clone() else {
            self.stack.set_visible_child_name("empty");
            return;
        };
        self.stack.set_visible_child_name("media");
        let label_text = state.label();
        self.title.set_text(if state.title.is_empty() { &label_text } else { &state.title });
        self.artist.set_text(&state.artist);
        self.album.set_text(&state.album);
        let playing = state.status == wm::PLAYING;
        self.play.set_label(if playing { ICON_PAUSE } else { ICON_PLAY });
        self.play.set_tooltip_text(Some(&if playing { tr("Pause") } else { tr("Play") }));
        self.play.set_sensitive(if playing { state.can_pause } else { state.can_play });
        self.previous.set_sensitive(state.can_previous);
        self.next.set_sensitive(state.can_next);
        if self.seek_hold.get().is_none_or(|until| Instant::now() >= until) {
            self.position_at.set((state.position, Instant::now()));
        }
        self.show_position();
        self.show_art(&state.art_url);
    }

    fn position_now(&self) -> i64 {
        let (position, at) = self.position_at.get();
        match &*self.current.borrow() {
            Some(s) if s.status == wm::PLAYING => position + at.elapsed().as_micros() as i64,
            _ => position,
        }
    }

    fn show_position(&self) {
        let current = self.current.borrow();
        let length = current.as_ref().map(|s| s.length).unwrap_or(0);
        let now = self.position_now();
        let position = if length > 0 { now.min(length) } else { now };
        self.scale.set_sensitive(current.as_ref().is_some_and(|s| s.can_seek) && length > 0);
        self.scale.set_range(0.0, length.max(1) as f64);
        self.scale.set_value(if length > 0 { position as f64 } else { 0.0 });
        self.elapsed.set_text(&wm::fmt_time(position));
        self.total.set_text(&if length > 0 { wm::fmt_time(length) } else { String::new() });
    }

    fn tick(&self) {
        if self.current.borrow().as_ref().is_some_and(|s| s.status == wm::PLAYING) {
            self.show_position();
        }
    }

    fn show_art(self: &Rc<Self>, url: &str) {
        if self.art_url.borrow().as_deref() == Some(url) {
            return;
        }
        self.art_url.replace(Some(url.to_string()));
        if !art::has_art(url) {
            self.show_cover_page("glyph");
            return;
        }
        // Arquivo ou rede, lidos e decodificados em outra linha; a capa anterior fica até a nova chegar
        let (me, url) = (Rc::downgrade(self), url.to_string());
        glib::MainContext::default().spawn_local(async move {
            let wanted = url.clone();
            let data = gio::spawn_blocking(move || art::load_art_public(&wanted)).await.ok().flatten();
            let Some(p) = me.upgrade() else { return };
            if p.art_url.borrow().as_deref() != Some(url.as_str()) {
                return;
            }
            match data.as_deref().and_then(cover_pixbuf) {
                None => p.show_cover_page("glyph"),
                Some(pixbuf) => {
                    let size = (pixbuf.width(), pixbuf.height());
                    if p.cover_size.replace(Some(size)) != Some(size) {
                        p.resized(); // só quando muda: cada resize abre a carência do focus-out
                    }
                    p.picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&pixbuf)));
                    p.picture.set_size_request(size.0, size.1);
                    p.show_cover_page("image");
                }
            }
        });
    }

    fn show_cover_page(&self, name: &str) {
        if self.cover.visible_child_name().as_deref() != Some(name) {
            self.resized();
        }
        self.cover.set_visible_child_name(name);
    }

    /// A capa mudou de altura: o popup segue. O focus-out que o Sway manda com o resize não fecha.
    fn resized(&self) {
        self.popup.rendered_at.set(Instant::now());
        // Encolhe até o conteúdo, e não só cresce
        self.popup.window.set_default_size(1, 1);
    }

    // --- ações ---
    fn send(self: &Rc<Self>, method: &'static str) {
        let Some(state) = self.current.borrow().clone() else { return };
        let (me, bus, name) = (Rc::downgrade(self), self.bus.clone(), state.name.clone());
        glib::MainContext::default().spawn_local(async move {
            let n = name.clone();
            let _ = gio::spawn_blocking(move || wm::command(&bus, &n, method)).await;
            if let Some(p) = me.upgrade() {
                p.refresh(Some(name));
            }
        });
    }

    fn on_seek(self: &Rc<Self>, value: f64) {
        let Some(state) = self.current.borrow().clone() else { return };
        if !state.can_seek || state.length <= 0 {
            return;
        }
        let target = (value as i64).clamp(0, state.length);
        self.seek_hold.set(Some(Instant::now() + SEEK_HOLD));
        self.position_at.set((target, Instant::now()));
        self.show_position();
        self.seek_target.replace(Some((state, target)));
        if self.seek_timer.borrow().is_none() {
            let me = Rc::downgrade(self);
            let timer = glib::timeout_add_local_once(SEEK_DELAY, move || {
                let Some(p) = me.upgrade() else { return };
                p.seek_timer.take();
                if let Some((state, target)) = p.seek_target.take() {
                    let bus = p.bus.clone();
                    gio::spawn_blocking(move || wm::seek_to(&bus, &state, target));
                }
            });
            self.seek_timer.replace(Some(timer));
        }
    }
}

fn notify_icon() {
    // O ícone esconde a dica com o popup aberto
    swaydesk_common::resident::notify_icons("waybar-media");
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-media-popup\n",
    "    Opens the media popup: cover, track, progress and the controls, through MPRIS (opening it again closes it).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-media-popup em Python confere que o binário carrega
    }
    run_popup(PROGRAM, Popup::new, notify_icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_fits_like_python() {
        assert_eq!(fit(600, 600), (200, 200)); // um disco
        assert_eq!(fit(1280, 720), (320, 180)); // um vídeo
        assert_eq!(fit(150, 83), (225, 125)); // pequena: até 1,5×
        assert_eq!(fit(0, 0), (1, 1));
    }
}

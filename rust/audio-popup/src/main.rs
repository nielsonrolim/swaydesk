//! Popup de áudio para o clique no volume da Waybar: o porte de `home/.local/bin/waybar-audio`, que passa
//! a vez a este binário (sway_rust).
//!
//! Volume e mudo da saída e do microfone, e a escolha do dispositivo de saída e de entrada, com o atual
//! na cor de destaque. Acompanha mudanças de fora (teclas de volume, fone conectado) pelo
//! `pactl subscribe`. Dados: `pw-dump`; ações: `wpctl` (volume, mudo, padrão) e `pactl set-sink-port`/
//! `set-source-port` (alto-falantes x fones na mesma placa).
mod pipewire;

use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib, pango};
use pipewire::{Entry, Kind, Node, State};
use swaydesk_common::i18n::tr;
use swaydesk_ui::cmd::spawn_detached;
use swaydesk_ui::list::{CSS_DEV, CSS_SCALE};
use swaydesk_ui::{log, run_popup, PopupWindow};

const PROGRAM: &str = "waybar-audio";
/// Altura fixa da superfície: o Sway tira o foco quando ela muda de tamanho, e sem foco o Esc e o clique
/// fora param de funcionar. Só o cartão recebe cliques (a região de entrada).
const SURFACE_HEIGHT: i32 = 900;
/// Novas leituras após uma falha, com espera crescente
const RETRIES: u32 = 5;
/// Quanto o volume espera por mais mudanças da escala antes de ir para o wpctl
const VOLUME_DELAY: Duration = Duration::from_millis(50);

const CSS: &str = r#"
.box    { background: #38383d; border-radius: 10px; padding: 6px; }
.pct    { font-size: 12px; min-width: 38px; }
button.mute {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  font-family: "Symbols Nerd Font"; font-size: 17px; padding: 0 6px; min-height: 0;
  border-radius: 8px;
}
button.mute:hover { background: #4c4c50; }
button.mute.muted { color: #8a8a8f; }
scale.muted highlight { background: #8a8a8f; }
button.dev {
  background: transparent; border: none; box-shadow: none; color: #dadada;
  padding: 6px 10px; border-radius: 8px; min-height: 0;
}
button.dev.active .dev-sub { color: @accent_soft; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
"#;

/// Material da Nerd Font: volume-high / volume-off, microphone / microphone-off
fn icon(kind: Kind, muted: bool) -> &'static str {
    match (kind, muted) {
        (Kind::Sink, false) => "\u{f057e}",
        (Kind::Sink, true) => "\u{f0581}",
        (Kind::Source, false) => "\u{f036c}",
        (Kind::Source, true) => "\u{f036d}",
    }
}

fn run(program: &str, args: &[&str]) {
    // Com prazo, na linha da interface, como o Python: o wpctl responde em milissegundos
    let ok = Command::new("timeout")
        .args(["-k", "1", "3", program])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(err) = ok {
        log(PROGRAM, &format!("{program}: {err}"));
    }
}

fn read_state() -> Result<State, String> {
    let out = Command::new("timeout").args(["-k", "1", "3", "pw-dump"]).stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || text.trim().is_empty() {
        // PipeWire fora do ar não é "sem dispositivos"
        return Err(format!("pw-dump failed ({})", out.status));
    }
    Ok(pipewire::read_state(&pipewire::parse_dump(&text)?))
}

/// Volume, mudo e a lista de dispositivos de um lado (saída ou entrada).
struct Section {
    kind: Kind,
    widget: gtk4::Box,
    mute: gtk4::Button,
    scale: gtk4::Scale,
    pct: gtk4::Label,
    list: gtk4::Box,
    updating: Cell<bool>,
    /// O botão do mouse segurado na escala (o arrasto do Python)
    pressed: Rc<Cell<bool>>,
    pending_volume: Cell<Option<i64>>,
    volume_timer: RefCell<Option<glib::SourceId>>,
    /// Quando o último volume chegou ao wpctl
    sent_at: Cell<Option<Instant>>,
    structure: RefCell<Option<Vec<(i64, Option<String>, String, String)>>>,
    buttons: RefCell<Vec<gtk4::Button>>,
}

impl Section {
    fn new(kind: Kind, title: &str) -> Rc<Section> {
        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        widget.append(&swaydesk_ui::label(title, &["section"], 0.0));
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let mute = gtk4::Button::with_label(icon(kind, false));
        mute.add_css_class("mute");
        mute.set_tooltip_text(Some(&tr("Mute")));
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        // O clique põe a escala num valor contínuo (73,6); arredondado já no clique, a leitura do
        // PipeWire (73) não a puxa de volta
        scale.set_round_digits(0);
        scale.set_hexpand(true);
        let pct = swaydesk_ui::label("", &["pct"], 1.0);
        row.append(&mute);
        row.append(&scale);
        row.append(&pct);
        widget.append(&row);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        list.add_css_class("box");
        widget.append(&list);
        let section = Rc::new(Section {
            kind,
            widget,
            mute,
            scale,
            pct,
            list,
            updating: Cell::new(false),
            pressed: Rc::new(Cell::new(false)),
            pending_volume: Cell::new(None),
            volume_timer: RefCell::new(None),
            sent_at: Cell::new(None),
            structure: RefCell::new(None),
            buttons: RefCell::new(Vec::new()),
        });
        let s = Rc::downgrade(&section);
        section.mute.connect_clicked(move |_| {
            if let Some(s) = s.upgrade() {
                run("wpctl", &["set-mute", s.target(), "toggle"]);
            }
        });
        let s = Rc::downgrade(&section);
        section.scale.connect_value_changed(move |scale| {
            if let Some(s) = s.upgrade() {
                s.on_scale(scale.value().round() as i64);
            }
        });
        // Só observa o botão, na captura: um gesto próprio é cancelado pelo arrasto do Scale
        let press = gtk4::EventControllerLegacy::new();
        press.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let pressed = section.pressed.clone();
        press.connect_event(move |_, event| {
            use gtk4::gdk::EventType as E;
            match event.event_type() {
                E::ButtonPress | E::TouchBegin => pressed.set(true),
                E::ButtonRelease | E::TouchEnd | E::TouchCancel | E::GrabBroken => pressed.set(false),
                _ => {}
            }
            glib::Propagation::Proceed
        });
        section.scale.add_controller(press);
        section
    }

    fn target(&self) -> &'static str {
        match self.kind {
            Kind::Sink => "@DEFAULT_AUDIO_SINK@",
            Kind::Source => "@DEFAULT_AUDIO_SOURCE@",
        }
    }

    fn on_scale(self: &Rc<Self>, value: i64) {
        self.pct.set_text(&format!("{value}%"));
        if self.updating.get() {
            return;
        }
        // No arrasto chegam dezenas de valores: manda só o último, sem travar a escala
        self.pending_volume.set(Some(value));
        if self.volume_timer.borrow().is_none() {
            let s = Rc::downgrade(self);
            let timer = glib::timeout_add_local_once(VOLUME_DELAY, move || {
                if let Some(s) = s.upgrade() {
                    s.volume_timer.take();
                    s.send_volume();
                }
            });
            self.volume_timer.replace(Some(timer));
        }
    }

    fn send_volume(&self) {
        if let Some(v) = self.pending_volume.take() {
            run("wpctl", &["set-volume", self.target(), &format!("{:.2}", v as f64 / 100.0)]);
            self.sent_at.set(Some(Instant::now()));
        }
    }

    fn on_pick(&self, entry: &Entry) {
        if let Some(port) = &entry.port {
            let command = if self.kind == Kind::Sink { "set-sink-port" } else { "set-source-port" };
            run("pactl", &[command, &entry.node.name, port]);
        }
        run("wpctl", &["set-default", &entry.node.id.to_string()]);
    }

    fn mark(button: &gtk4::Button, active: bool) {
        if active {
            button.add_css_class("active");
        } else {
            button.remove_css_class("active");
        }
    }

    /// `started`: quando a leitura começou. Uma que começou antes do último volume mandado traz o valor
    /// velho: o mudo e a lista valem, o volume não (a leitura do próprio wpctl vem logo).
    fn update(self: &Rc<Self>, entries: &[Entry], current: Option<&Node>, empty: &str, started: Option<Instant>) {
        // Volume e mudo: atualizados no lugar (sem mudar o tamanho do popup)
        self.updating.set(true);
        if let Some(current) = current {
            let stale = matches!((started, self.sent_at.get()), (Some(s), Some(sent)) if s < sent);
            if !self.pressed.get() && self.pending_volume.get().is_none() && !stale {
                self.scale.set_value(current.vol as f64);
                self.pct.set_text(&format!("{}%", current.vol));
            }
            self.mute.set_label(icon(self.kind, current.mute));
            for w in [self.mute.upcast_ref::<gtk4::Widget>(), self.scale.upcast_ref()] {
                if current.mute {
                    w.add_css_class("muted");
                } else {
                    w.remove_css_class("muted");
                }
            }
            self.mute.set_tooltip_text(Some(&if current.mute { tr("Unmute") } else { tr("Mute") }));
        }
        self.scale.set_sensitive(current.is_some());
        self.mute.set_sensitive(current.is_some());
        self.updating.set(false);

        // Trocar o padrão só muda a marcação; a lista é refeita se os itens mudam
        let structure: Vec<(i64, Option<String>, String, String)> = if entries.is_empty() {
            vec![(-1, None, empty.to_string(), String::new())]
        } else {
            entries.iter().map(|e| (e.node.id, e.port.clone(), e.title.clone(), e.sub.clone())).collect()
        };
        if self.structure.borrow().as_ref() == Some(&structure) {
            for (b, e) in self.buttons.borrow().iter().zip(entries) {
                Self::mark(b, e.active);
            }
            return;
        }
        self.structure.replace(Some(structure));
        self.buttons.borrow_mut().clear();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if entries.is_empty() {
            self.list.append(&swaydesk_ui::label(empty, &["dev-sub"], 0.0));
        }
        for e in entries {
            let b = gtk4::Button::new();
            b.add_css_class("dev");
            Self::mark(&b, e.active);
            let col = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
            let title = swaydesk_ui::label(&e.title, &["dev-name"], 0.0);
            title.set_ellipsize(pango::EllipsizeMode::End);
            title.set_max_width_chars(1); // nome longo não alarga o cartão
            col.append(&title);
            if !e.sub.is_empty() {
                let sub = swaydesk_ui::label(&e.sub, &["dev-sub"], 0.0);
                sub.set_ellipsize(pango::EllipsizeMode::End);
                sub.set_max_width_chars(1);
                col.append(&sub);
            }
            b.set_child(Some(&col));
            let (s, e) = (Rc::downgrade(self), e.clone());
            b.connect_clicked(move |_| {
                if let Some(s) = s.upgrade() {
                    s.on_pick(&e);
                }
            });
            self.list.append(&b);
            self.buttons.borrow_mut().push(b);
        }
    }
}

struct Popup {
    popup: Rc<PopupWindow>,
    card: gtk4::Box,
    out: Rc<Section>,
    inp: Rc<Section>,
    pending: Cell<bool>,
    reading: Cell<bool>,
    again: Cell<bool>,
    failures: Cell<u32>,
    has_data: Cell<bool>,
    shown: Cell<bool>,
    /// A região de entrada atual (x, y, largura, altura)
    input_rect: Cell<Option<(i32, i32, i32, i32)>>,
}

impl Popup {
    fn new(main_loop: &glib::MainLoop) -> Rc<Popup> {
        let popup = PopupWindow::new(PROGRAM, "waybar-audio", &format!("{CSS_SCALE}{CSS_DEV}{CSS}"), true, main_loop);
        let card = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        card.add_css_class("card");
        card.set_size_request(340, -1);
        card.set_valign(gtk4::Align::Start);
        popup.window.set_size_request(-1, SURFACE_HEIGHT);
        popup.window.set_child(Some(&card));

        let out = Section::new(Kind::Sink, &tr("Output"));
        let inp = Section::new(Kind::Source, &tr("Input"));
        card.append(&out.widget);
        card.append(&inp.widget);
        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        let more = gtk4::Button::with_label(&tr("More options"));
        more.add_css_class("link");
        more.set_tooltip_text(Some(&tr("Open the volume control (pavucontrol)")));
        foot.append(&more);
        card.append(&foot);

        let this = Rc::new(Popup {
            popup,
            card,
            out,
            inp,
            pending: Cell::new(false),
            reading: Cell::new(false),
            again: Cell::new(false),
            failures: Cell::new(0),
            has_data: Cell::new(false),
            shown: Cell::new(false),
            input_rect: Cell::new(None),
        });
        let me = Rc::downgrade(&this);
        more.connect_clicked(move |_| {
            let _ = spawn_detached("pavucontrol", &[] as &[&str]);
            if let Some(p) = me.upgrade() {
                p.popup.close("pavucontrol");
            }
        });
        this.refresh();
        this.watch_pulse();
        this
    }

    /// A parte transparente da superfície deixa os cliques passarem: a região de entrada é só o cartão.
    /// Recalculada no fim de cada layout do frame clock (o size-allocate do Python), só quando o retângulo
    /// muda: o cartão cresce com a lista (um fone conectado).
    fn shape_on_layout(self: &Rc<Self>) {
        let Some(clock) = self.popup.window.frame_clock() else { return };
        let me = Rc::downgrade(self);
        clock.connect_layout(move |_| {
            let Some(p) = me.upgrade() else { return };
            let (Some(surface), Some(bounds)) = (p.popup.window.surface(), p.card.compute_bounds(&p.popup.window)) else { return };
            let rect = (bounds.x().floor() as i32, bounds.y().floor() as i32, bounds.width().ceil() as i32, bounds.height().ceil() as i32);
            if p.input_rect.replace(Some(rect)) != Some(rect) {
                let r = gtk4::cairo::RectangleInt::new(rect.0, rect.1, rect.2, rect.3);
                surface.set_input_region(&gtk4::cairo::Region::create_rectangle(&r));
            }
        });
    }

    /// Lê o estado fora da linha do GTK: um PipeWire travado não trava o popup.
    fn refresh(self: &Rc<Self>) {
        self.pending.set(false);
        if self.reading.replace(true) {
            self.again.set(true);
            return;
        }
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let started = Instant::now();
            let result = gio::spawn_blocking(read_state).await.unwrap_or_else(|_| Err("panic".into()));
            if let Some(p) = me.upgrade() {
                p.apply(result, started);
            }
        });
    }

    fn apply(self: &Rc<Self>, result: Result<State, String>, started: Instant) {
        self.reading.set(false);
        match result {
            Ok(state) => {
                self.failures.set(0);
                self.has_data.set(true);
                let empty = tr("No device");
                self.out.update(&state.sinks, state.current_sink.as_ref(), &empty, Some(started));
                self.inp.update(&state.sources, state.current_source.as_ref(), &empty, Some(started));
            }
            Err(err) => {
                self.failures.set(self.failures.get() + 1);
                log(PROGRAM, &format!("leitura falhou ({}): {err}", self.failures.get()));
                if !self.has_data.get() {
                    let empty = tr("Audio unavailable");
                    self.out.update(&[], None, &empty, None);
                    self.inp.update(&[], None, &empty, None);
                }
                if self.failures.get() <= RETRIES && !self.pending.replace(true) {
                    let wait = (300u64 << (self.failures.get() - 1)).min(5000);
                    let me = Rc::downgrade(self);
                    glib::timeout_add_local_once(Duration::from_millis(wait), move || {
                        if let Some(p) = me.upgrade() {
                            p.refresh();
                        }
                    });
                }
            }
        }
        if !self.shown.replace(true) {
            self.popup.show();
            self.shape_on_layout();
        }
        if self.again.replace(false) {
            self.schedule();
        }
    }

    /// Agrupa a rajada de eventos num refresh só.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let me = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if let Some(p) = me.upgrade() {
                    p.refresh();
                }
            });
        }
    }

    /// Acompanha mudanças pelo `pactl subscribe`; se o servidor de áudio reiniciar, reconecta e relê.
    fn watch_pulse(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            loop {
                // setpriv: o pactl morre com o popup, mesmo num SIGKILL (ele ignora o SIGPIPE)
                let child = Command::new("setpriv")
                    .args(["--pdeathsig", "TERM", "pactl", "subscribe"])
                    .env("LC_ALL", "C")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                let mut child = match child {
                    Ok(child) => child,
                    Err(err) => {
                        log(PROGRAM, &format!("pactl subscribe: {err}"));
                        return;
                    }
                };
                let Some(stdout) = child.stdout.take() else { return };
                let mut reader = BufReader::new(stdout);
                loop {
                    // Numa linha bloqueante do GIO, até uma mudança que interesse ou o fim
                    let Ok((back, changed)) = gio::spawn_blocking(move || {
                        // Em bytes, com o UTF-8 inválido trocado (o errors="replace" do Python): um erro de
                        // texto não pode encerrar o laço com o pactl ainda vivo
                        let mut line = Vec::new();
                        loop {
                            line.clear();
                            match reader.read_until(b'\n', &mut line) {
                                Ok(0) | Err(_) => return (reader, false),
                                Ok(_) if relevant(&String::from_utf8_lossy(&line)) => return (reader, true),
                                Ok(_) => {}
                            }
                        }
                    })
                    .await
                    else {
                        return; // a linha de leitura caiu
                    };
                    reader = back;
                    if !changed {
                        break;
                    }
                    match me.upgrade() {
                        Some(p) => p.schedule(),
                        None => return,
                    }
                }
                // Fim da leitura com o pactl ainda vivo (um erro de E/S): encerra antes de esperar
                let _ = child.kill();
                let _ = gio::spawn_blocking(move || child.wait()).await;
                glib::timeout_future(Duration::from_secs(2)).await;
                match me.upgrade() {
                    Some(p) => p.schedule(),
                    None => return,
                }
            }
        });
    }
}

/// Uma linha do `pactl subscribe` que muda o que o popup mostra (não os fluxos dos apps).
fn relevant(line: &str) -> bool {
    ["sink", "source", "server", "card"].iter().any(|k| line.contains(k)) && !line.contains("sink-input") && !line.contains("source-output")
}

fn notify_icon() {
    // O ícone do volume esconde a dica com o popup aberto
    swaydesk_common::resident::notify_icons("waybar-volume");
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-audio-popup\n",
    "    Opens the audio popup: volume, mute, and the output and input devices (opening it again closes it).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-audio em Python confere que o binário carrega
    }
    run_popup(PROGRAM, |main_loop| Some(Popup::new(main_loop)), notify_icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_device_events_matter() {
        assert!(relevant("Event 'change' on sink #56"));
        assert!(relevant("Event 'change' on card #40"));
        assert!(relevant("Event 'change' on server #-1"));
        assert!(!relevant("Event 'change' on sink-input #120"));
        assert!(!relevant("Event 'new' on source-output #7"));
        assert!(!relevant("Event 'new' on client #9"));
    }
}

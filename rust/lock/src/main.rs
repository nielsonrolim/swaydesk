//! A tela de bloqueio em Rust + GTK 4: o porte do `sway-lock-gtk`, pelo ext-session-lock-v1 do
//! gtk4-layer-shell 1.2 ou mais novo ([`session_lock`]).
//!
//! Não abrir direto: quem abre é o sway-lock, pelo supervisor sway-lock-watch, com --notify-fd N. No fd N
//! este programa escreve "L" ao receber locked, "F" se recebeu failed antes (outro já bloqueou) e "U" só
//! depois do unlock e de um roundtrip com o Sway. Sair de qualquer outro jeito deixa a tela bloqueada, e
//! o supervisor abre o swaylock. Sem o bloqueio no gtk4-layer-shell (o 1.0.4 do Debian 13), sai com 1 e o
//! sway-lock passa ao sway-lock-gtk.
//!
//! Segurança, como no Python:
//! - a senha vai do campo (o GtkPasswordEntry guarda o texto em memória que não vai para o swap) para um
//!   Vec que o PAM zera; o campo é limpo na hora;
//! - o PAM roda numa linha à parte, e só o pam_authenticate ([`swaydesk_lock_logic::pam`]);
//! - o fundo desfocado só é lido do cache antes do locked: sem ele, um fundo liso, e o desfoque é gerado
//!   depois;
//! - o campo de senha aparece em todos os monitores: se o principal sumir, ainda há onde digitar.
//!
//! Também gera de antemão o fundo desfocado de cada monitor ligado (`--blur`, ou chamado como
//! `sway-lock-blur`): o porte do `sway-lock-blur`, que o bing-wallpaper chama depois de trocar a imagem.
//! O bloqueio só lê esse cache. Sem o Sway ou sem papel de parede, não faz nada e sai com 0.
//!
//! uso: swaydesk-lock --notify-fd N | --check | --blur
mod blur;
mod session_lock;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gdk_pixbuf, gio, glib};
use swaydesk_common::config;
use swaydesk_common::i18n::tr;
use swaydesk_lock_logic::{self as sl, pam};
use swaydesk_media as media;

const AVATAR: i32 = 96;
const PHOTO_MAX_BYTES: u64 = 2 * 1024 * 1024; // a única entrada de tamanho livre: uma foto maior fica nas iniciais
const FAIL_LIMIT: u32 = 3; // erros seguidos antes da espera
const FAIL_WAIT: Duration = Duration::from_secs(5); // a espera, além da que o próprio PAM fizer
const PAM_STUCK_S: u32 = 30; // um PAM que não responde (rede, leitor de digital): o campo volta
const SHAKE_MS: f64 = 400.0;
const STATE_EVERY_S: u32 = 2; // bateria, rede, layout e mídia, numa linha à parte
const ART: i32 = 64;
const SWAYMSG_TIMEOUT: Duration = Duration::from_secs(2);
const BLUR_KEEP: usize = 4;
// Nerd Font: anterior, tocar, pausar e próxima (os do popup de mídia)
const MEDIA_PREV: &str = "\u{f04ae}";
const MEDIA_PLAY: &str = "\u{f040a}";
const MEDIA_PAUSE: &str = "\u{f03e4}";
const MEDIA_NEXT: &str = "\u{f04ad}";

// O do Python. O GTK 4 não tem as animações do CSS: o tremor do campo é feito pelas margens ([`shake`])
const CSS: &str = r#"
window { background: #1e1e22; }
.time { color: #ffffff; font-size: 84px; font-weight: 300; }
.date { color: #e6e6e6; font-size: 20px; }
.name { color: #ffffff; font-size: 18px; font-weight: bold; }
.avatar { min-width: 96px; min-height: 96px; border-radius: 48px; background: @accent; }
.initials { color: @on_accent; font-size: 34px; font-weight: bold; }
entry.lock-password {
  min-width: 260px; min-height: 36px; padding: 0 12px; border-radius: 18px;
  background: rgba(46, 46, 51, 0.85); color: #ffffff; border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: none; outline: none; caret-color: #ffffff;
}
entry.lock-password:focus-within { border-color: @accent; }
.status { color: #f2c94c; font-size: 13px; min-height: 18px; }
.status.error { color: #ff8a80; }
.corner { color: #ffffff; font-size: 14px; }
.glyph { font-family: "Symbols Nerd Font"; font-size: 16px; }
button.flat-btn {
  background: transparent; border: none; box-shadow: none; color: #ffffff; padding: 2px 8px;
  border-radius: 8px; min-height: 0;
}
button.flat-btn:hover { background: rgba(255, 255, 255, 0.12); }
.media { background: rgba(46, 46, 51, 0.85); border-radius: 12px; padding: 10px 14px; color: #ffffff; }
.media-title { font-weight: bold; font-size: 14px; }
.media-artist { color: #c8c8c8; font-size: 12px; }
"#;

fn log(message: &str) {
    eprintln!("swaydesk-lock: {message}");
}

/// Zera a memória com escritas que o compilador não tira.
unsafe fn zero(start: *mut u8, len: usize) {
    for i in 0..len {
        std::ptr::write_volatile(start.add(i), 0);
    }
}

/// O pipe do supervisor. Um erro de escrita não derruba o bloqueio: quem decide é o supervisor.
struct Notifier(i32);

impl Notifier {
    fn send(&self, code: &[u8]) {
        unsafe { libc::write(self.0, code.as_ptr().cast(), code.len()) };
    }
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::new(None);
    label.add_css_class(class);
    label
}

fn flat_button(text: &str, glyph: bool) -> gtk4::Button {
    let button = gtk4::Button::with_label(text);
    button.add_css_class("flat-btn");
    if glyph {
        button.add_css_class("glyph");
    }
    button.set_focusable(false);
    button.set_focus_on_click(false);
    button
}

/// O estado do canto e da mídia, lido numa linha à parte.
#[derive(Clone, Default)]
struct SystemState {
    battery: Option<(u32, sl::Power)>,
    net: Option<sl::Net>,
    layout: (String, bool),
    media: Option<media::State>,
}

/// A tela de um monitor: fundo, hora, data, foto, nome, campo de senha, a linha de aviso, o canto e a mídia.
struct LockWindow {
    window: gtk4::Window,
    /// O tamanho do monitor; 0x0 num monitor religado, até o GDK saber a geometria
    size: Cell<(i32, i32)>,
    background: gtk4::Picture,
    time: gtk4::Label,
    date: gtk4::Label,
    photo: gtk4::Picture,
    letters: gtk4::Label,
    entry: gtk4::PasswordEntry,
    status: gtk4::Label,
    corner: gtk4::Box,
    battery: gtk4::Label,
    battery_text: gtk4::Label,
    net: gtk4::Label,
    layout: gtk4::Button,
    card: gtk4::Box,
    art: gtk4::Picture,
    title: gtk4::Label,
    artist: gtk4::Label,
    media_buttons: [gtk4::Button; 3],
}

impl LockWindow {
    fn new(app: &Rc<App>, monitor: &gdk::Monitor) -> Rc<LockWindow> {
        let geometry = monitor.geometry();
        let window = gtk4::Window::new();
        let overlay = gtk4::Overlay::new();
        // Liso até o locked: o PNG do desfoque é lido depois, numa linha à parte. O Sway só manda o locked
        // com todas as superfícies desenhadas, e isso conta no prazo do sway-lock
        let background = gtk4::Picture::new();
        background.set_content_fit(gtk4::ContentFit::Cover);
        background.set_can_shrink(true);
        overlay.set_child(Some(&background));

        let center = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        center.set_halign(gtk4::Align::Center);
        center.set_valign(gtk4::Align::Center);
        let time = label("time");
        let date = label("date");
        center.append(&time);
        center.append(&date);
        let avatar = gtk4::Overlay::new();
        avatar.add_css_class("avatar");
        avatar.set_overflow(gtk4::Overflow::Hidden);
        avatar.set_halign(gtk4::Align::Center);
        avatar.set_margin_top(36);
        let photo = gtk4::Picture::new();
        photo.set_content_fit(gtk4::ContentFit::Cover);
        photo.set_size_request(AVATAR, AVATAR);
        photo.set_visible(false);
        avatar.set_child(Some(&photo));
        let letters = label("initials");
        letters.set_text(&app.initials);
        letters.set_visible(!app.initials.is_empty());
        avatar.add_overlay(&letters);
        center.append(&avatar);
        let name = label("name");
        name.set_text(&app.name);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name.set_max_width_chars(40);
        center.append(&name);
        let entry = gtk4::PasswordEntry::new();
        entry.set_property("placeholder-text", tr("Password"));
        entry.add_css_class("lock-password");
        entry.set_halign(gtk4::Align::Center);
        entry.set_margin_top(8);
        {
            let app = Rc::downgrade(app);
            entry.connect_activate(move |entry| {
                if let Some(app) = app.upgrade() {
                    app.submit(entry);
                }
            });
        }
        center.append(&entry);
        let status = label("status");
        center.append(&status);
        overlay.add_overlay(&center);

        // No canto superior direito: bateria, rede e o layout do teclado, que troca com um clique
        let corner = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
        corner.add_css_class("corner");
        corner.set_halign(gtk4::Align::End);
        corner.set_valign(gtk4::Align::Start);
        corner.set_margin_top(20);
        corner.set_margin_end(24);
        corner.set_visible(false);
        let battery = label("glyph");
        let battery_text = gtk4::Label::new(None);
        let net = label("glyph");
        let layout = flat_button("", false);
        {
            let app = Rc::downgrade(app);
            layout.connect_clicked(move |_| {
                if let Some(app) = app.upgrade() {
                    app.switch_layout();
                }
            });
        }
        for widget in [battery.upcast_ref::<gtk4::Widget>(), battery_text.upcast_ref(), net.upcast_ref(), layout.upcast_ref()] {
            corner.append(widget);
        }
        overlay.add_overlay(&corner);

        // No canto inferior esquerdo, só com mídia tocando ou pausada
        let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        card.add_css_class("media");
        card.set_halign(gtk4::Align::Start);
        card.set_valign(gtk4::Align::End);
        card.set_margin_start(24);
        card.set_margin_bottom(24);
        card.set_visible(false);
        let art = gtk4::Picture::new();
        art.set_size_request(ART, ART);
        art.set_content_fit(gtk4::ContentFit::Cover);
        art.set_visible(false);
        card.append(&art);
        let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        text.set_valign(gtk4::Align::Center);
        let title = label("media-title");
        let artist = label("media-artist");
        for l in [&title, &artist] {
            l.set_xalign(0.0);
            l.set_max_width_chars(32);
            l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            text.append(l);
        }
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let media_buttons = [("Previous", MEDIA_PREV), ("PlayPause", MEDIA_PLAY), ("Next", MEDIA_NEXT)].map(|(method, glyph)| {
            let button = flat_button(glyph, true);
            let app = Rc::downgrade(app);
            button.connect_clicked(move |_| {
                if let Some(app) = app.upgrade() {
                    app.media_command(method);
                }
            });
            buttons.append(&button);
            button
        });
        text.append(&buttons);
        card.append(&text);
        overlay.add_overlay(&card);

        window.set_child(Some(&overlay));
        let lock_window = Rc::new(LockWindow {
            window,
            size: Cell::new((geometry.width(), geometry.height())),
            background,
            time,
            date,
            photo,
            letters,
            entry,
            status,
            corner,
            battery,
            battery_text,
            net,
            layout,
            card,
            art,
            title,
            artist,
            media_buttons,
        });
        lock_window.refresh_clock(&app.time_locale);
        lock_window
    }

    fn show_system(&self, state: &SystemState, main: bool) {
        self.corner.set_visible(main);
        if !main {
            return;
        }
        match state.battery {
            Some((level, power)) => {
                self.battery.set_text(sl::battery_icon(level, power));
                self.battery_text.set_text(&format!("{level}%"));
            }
            None => {}
        }
        self.battery.set_visible(state.battery.is_some());
        self.battery_text.set_visible(state.battery.is_some());
        self.net.set_text(sl::network_icon(state.net.unwrap_or(sl::Net::Offline)));
        let (name, many) = &state.layout;
        self.layout.set_label(name);
        self.layout.set_visible(!name.is_empty());
        self.layout.set_sensitive(*many);
    }

    fn show_media(&self, state: Option<&media::State>, art: Option<&gdk::Texture>, main: bool) {
        let Some(media) = state.filter(|_| main) else {
            self.card.set_visible(false);
            return;
        };
        self.title.set_text(if media.title.is_empty() { media.label() } else { media.title.clone() }.as_str());
        self.artist.set_text(&media.artist);
        self.artist.set_visible(!media.artist.is_empty());
        self.media_buttons[1].set_label(if media.status == media::PLAYING { MEDIA_PAUSE } else { MEDIA_PLAY });
        self.media_buttons[0].set_sensitive(media.can_previous);
        self.media_buttons[2].set_sensitive(media.can_next);
        self.art.set_paintable(art);
        self.art.set_visible(art.is_some());
        self.card.set_visible(true);
    }

    fn refresh_clock(&self, time_locale: &str) {
        let now = sl::local_now();
        self.time.set_text(&sl::time_text(&now));
        self.date.set_text(&sl::date_text(&now, time_locale));
    }

    fn set_status(&self, text: &str, error: bool) {
        self.status.set_text(text);
        if error {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
    }

    fn set_photo(&self, texture: &gdk::Texture) {
        self.photo.set_paintable(Some(texture));
        self.photo.set_visible(true);
        self.letters.set_visible(false);
    }

    /// O tremor do campo: as margens dos quadros do Python (0, 20, 40, 60, 80 e 100%), em 400 ms.
    fn shake(&self) {
        const FRAMES: [(f64, f64); 6] = [(0.0, 0.0), (16.0, 0.0), (0.0, 16.0), (12.0, 0.0), (0.0, 8.0), (0.0, 0.0)];
        let entry = self.entry.clone();
        let start = Instant::now();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let t = (start.elapsed().as_secs_f64() * 1000.0 / SHAKE_MS).min(1.0);
            let position = t * 5.0;
            let i = (position.floor() as usize).min(4);
            let f = position - i as f64;
            let (a, b) = (FRAMES[i], FRAMES[i + 1]);
            entry.set_margin_start((a.0 + (b.0 - a.0) * f).round() as i32);
            entry.set_margin_end((a.1 + (b.1 - a.1) * f).round() as i32);
            if t >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
        });
    }
}

struct App {
    notifier: Notifier,
    main_loop: glib::MainLoop,
    instance: session_lock::Instance,
    display: gdk::Display,
    locked: Cell<bool>,
    /// O laço já foi mandado parar (um ::failed pode vir de dentro do lock(), antes do run())
    done: Cell<bool>,
    unlocking: Cell<bool>,
    busy: Cell<bool>,
    /// A resposta de uma verificação abandonada (PAM travado) é ignorada
    attempt: Cell<u64>,
    failures: Cell<u32>,
    wait_until: Cell<Option<Instant>>,
    /// Na ordem em que os monitores chegaram: o primeiro é o principal (o canto e a mídia)
    windows: RefCell<Vec<Rc<LockWindow>>>,
    name: String,
    initials: String,
    user: String,
    photo_path: Option<PathBuf>,
    photo: RefCell<Option<gdk::Texture>>,
    time_locale: String,
    state: RefCell<SystemState>,
    art_url: RefCell<String>,
    art: RefCell<Option<gdk::Texture>>,
}

/// O barramento da sessão, para a mídia; sem ele, sem mídia.
fn session_bus() -> Option<gio::DBusConnection> {
    static BUS: OnceLock<Option<gio::DBusConnection>> = OnceLock::new();
    BUS.get_or_init(|| gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()).clone()
}

/// O filho começa sem sinais bloqueados. As linhas do GIO (o spawn_blocking) bloqueiam todos, e o
/// Command da biblioteca padrão passa a máscara adiante: um swaymsg com o SIGTERM bloqueado não sairia
/// com um pkill e ficaria órfão.
fn unblocked(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    // SAFETY: só chamadas async-signal-safe entre o fork e o exec
    unsafe {
        command.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            Ok(())
        })
    }
}

/// A saída de um comando que termina com 0 dentro do prazo; senão None, e ele é morto. Um swaymsg preso
/// (o Sway ocupado) não pode segurar uma linha do pool nem congelar o laço do canto, como o timeout=2 do
/// Python. A saída é lida por outra linha: um pipe cheio não trava a espera.
fn output_within(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = unblocked(command).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut data);
        data
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // Morto no prazo, um neto ainda pode segurar o pipe: a linha de leitura fica, e a resposta é None
    let status = status?;
    let data = reader.join().ok()?;
    status.success().then_some(data)
}

/// Bateria, rede, layout e mídia. Bloqueia: numa linha à parte.
fn read_state() -> SystemState {
    let inputs = output_within(Command::new("swaymsg").args(["-r", "-t", "get_inputs"]), SWAYMSG_TIMEOUT)
        .and_then(|out| serde_json::from_slice(&out).ok())
        .unwrap_or(serde_json::Value::Null);
    let media = session_bus().and_then(|bus| {
        let states = media::read_all(&bus, &|_| (None, None));
        media::pick(&states, None).cloned()
    });
    SystemState {
        battery: sl::battery(Path::new(sl::POWER_SUPPLY)),
        net: Some(sl::network(Path::new(sl::SYS_NET), Path::new(sl::PROC_WIRELESS))),
        layout: sl::keyboard_layout(&inputs),
        media,
    }
}

/// A capa: só local ou embutida, e só com o carregador do formato que os primeiros bytes dizem.
fn load_art(url: &str) -> Option<gdk::Texture> {
    let (data, kind) = sl::art_bytes(url, &media::art::art_cache())?;
    let loader = gdk_pixbuf::PixbufLoader::with_type(kind).ok()?;
    loader.set_size(ART, ART);
    let written = loader.write(&data);
    let closed = loader.close();
    written.ok()?;
    closed.ok()?;
    Some(gdk::Texture::for_pixbuf(&loader.pixbuf()?))
}

/// A foto no tamanho do avatar, ou None.
fn load_photo(path: &Path) -> Option<gdk::Texture> {
    if std::fs::metadata(path).ok()?.len() > PHOTO_MAX_BYTES {
        return None;
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, AVATAR, AVATAR, true).ok()?;
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

/// O fundo do monitor: o do cache ou, se faltar, gerado agora (depois do locked).
fn load_background(size: (i32, i32)) -> Option<gdk::Texture> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let wallpaper = config::home().join(sl::WALLPAPER);
    let path = sl::cached_blur(&wallpaper, size.0, size.1, &env).or_else(|| blur::make_blur(&wallpaper, size.0, size.1, &env, BLUR_KEEP))?;
    gdk::Texture::from_filename(path).ok()
}

impl App {
    fn add_monitor(self: &Rc<Self>, monitor: &gdk::Monitor) {
        if self.done.get() {
            return;
        }
        let window = LockWindow::new(self, monitor);
        self.instance.assign(&window.window, monitor);
        if self.done.get() {
            // O ::failed veio no meio: a biblioteca já destruiu a janela
            return;
        }
        let (width, height) = window.size.get();
        log(&format!("monitor {} ({width}x{height})", monitor.connector().unwrap_or_default()));
        window.window.present();
        if let Some(photo) = self.photo.borrow().as_ref() {
            window.set_photo(photo);
        }
        // O monitor que sai: a biblioteca desmapeia a janela (sem destruí-la), e ela sai da lista. A
        // primeira que sobrar vira a principal
        {
            let app = Rc::downgrade(self);
            let gone = Rc::downgrade(&window);
            monitor.connect_invalidate(move |monitor| {
                let (Some(app), Some(gone)) = (app.upgrade(), gone.upgrade()) else { return };
                if app.unlocking.get() {
                    return;
                }
                log(&format!("monitor {} gone", monitor.connector().unwrap_or_default()));
                let was_main = app.windows.borrow().first().is_some_and(|w| Rc::ptr_eq(w, &gone));
                app.windows.borrow_mut().retain(|w| !Rc::ptr_eq(w, &gone));
                if was_main && app.locked.get() && !app.unlocking.get() {
                    app.apply_state();
                }
            });
        }
        // Um monitor religado chega com 0x0, e a geometria vem depois, sem aviso: o fundo espera por ela
        // (até 5 s)
        if window.size.get().0 <= 0 || window.size.get().1 <= 0 {
            let app = Rc::downgrade(self);
            let waiting = Rc::downgrade(&window);
            let monitor = monitor.clone();
            glib::spawn_future_local(async move {
                for _ in 0..50 {
                    glib::timeout_future(Duration::from_millis(100)).await;
                    let (Some(app), Some(window)) = (app.upgrade(), waiting.upgrade()) else { return };
                    let geometry = monitor.geometry();
                    if geometry.width() > 0 && geometry.height() > 0 {
                        let size = (geometry.width(), geometry.height());
                        window.size.set(size);
                        if app.locked.get() {
                            app.load_backgrounds(vec![size]);
                        }
                        return;
                    }
                }
            });
        }
        self.windows.borrow_mut().push(window.clone());
        window.entry.grab_focus();
        self.show_caps();
        if self.locked.get() {
            // Um monitor ligado com a tela já bloqueada
            if window.size.get().0 > 0 {
                self.load_backgrounds(vec![window.size.get()]);
            }
            self.apply_state();
        }
    }

    fn tick(&self) {
        for window in self.windows.borrow().iter() {
            window.refresh_clock(&self.time_locale);
        }
        if self.wait_until.get().is_some_and(|until| Instant::now() >= until) {
            self.wait_until.set(None);
            self.set_sensitive(true);
            self.show_caps();
        }
    }

    fn on_locked(self: &Rc<Self>) {
        self.locked.set(true);
        self.notifier.send(b"L");
        // Agora, e não antes: a foto, o fundo do cache e o desfoque que faltar, fora da linha principal
        if let Some(path) = self.photo_path.clone() {
            let app = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let photo = gio::spawn_blocking(move || load_photo(&path)).await.ok().flatten();
                if let (Some(app), Some(photo)) = (app.upgrade(), photo) {
                    for window in app.windows.borrow().iter() {
                        window.set_photo(&photo);
                    }
                    *app.photo.borrow_mut() = Some(photo);
                }
            });
        }
        let mut sizes: Vec<(i32, i32)> = Vec::new();
        for window in self.windows.borrow().iter() {
            if !sizes.contains(&window.size.get()) {
                sizes.push(window.size.get());
            }
        }
        self.load_backgrounds(sizes);
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            loop {
                let Some(this) = app.upgrade() else { return };
                this.refresh_state().await;
                drop(this);
                glib::timeout_future_seconds(STATE_EVERY_S).await;
            }
        });
    }

    fn load_backgrounds(self: &Rc<Self>, sizes: Vec<(i32, i32)>) {
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            for size in sizes {
                let texture = gio::spawn_blocking(move || load_background(size)).await.ok().flatten();
                let (Some(app), Some(texture)) = (app.upgrade(), texture) else { continue };
                for window in app.windows.borrow().iter().filter(|w| w.size.get() == size) {
                    window.background.set_paintable(Some(&texture));
                    log(&format!("background {}x{}", size.0, size.1));
                }
            }
        });
    }

    /// Um /sys ou um get_inputs inesperado não pode parar o laço: o canto e a mídia ficariam congelados.
    async fn refresh_state(self: &Rc<Self>) {
        let state = gio::spawn_blocking(read_state).await.unwrap_or_default();
        let url = state.media.as_ref().map(|m| m.art_url.clone()).unwrap_or_default();
        if url != *self.art_url.borrow() {
            *self.art_url.borrow_mut() = url.clone();
            let art = gio::spawn_blocking(move || load_art(&url)).await.ok().flatten();
            *self.art.borrow_mut() = art;
        }
        *self.state.borrow_mut() = state;
        self.apply_state();
    }

    fn apply_state(&self) {
        let state = self.state.borrow();
        let art = self.art.borrow();
        for (index, window) in self.windows.borrow().iter().enumerate() {
            window.show_system(&state, index == 0);
            window.show_media(state.media.as_ref(), art.as_ref(), index == 0);
        }
    }

    fn refresh_soon(self: &Rc<Self>, delay: Duration) {
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(delay).await;
            if let Some(app) = app.upgrade() {
                app.refresh_state().await;
            }
        });
    }

    fn switch_layout(self: &Rc<Self>) {
        let _ = unblocked(&mut Command::new("swaymsg"))
            .args(["input", "type:keyboard", "xkb_switch_layout", "next"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut child| std::thread::spawn(move || child.wait()));
        self.refresh_soon(Duration::from_millis(300));
    }

    fn media_command(self: &Rc<Self>, method: &'static str) {
        let Some(name) = self.state.borrow().media.as_ref().map(|m| m.name.clone()) else { return };
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || session_bus().is_some_and(|bus| media::command(&bus, &name, method))).await;
            if !matches!(done, Ok(true)) {
                log(&format!("media {method}: no answer"));
            }
            glib::timeout_future(Duration::from_millis(200)).await;
            if let Some(app) = app.upgrade() {
                app.refresh_state().await;
            }
        });
    }

    fn on_failed(&self) {
        if !self.locked.get() {
            // Outro cliente já bloqueou: a tela está bloqueada, e não por nós
            self.notifier.send(b"F");
        }
        self.quit();
    }

    fn on_unlocked(&self) {
        if !self.unlocking.get() {
            // O Sway encerrou o nosso bloqueio sem desbloquear: sair sem "U", e o supervisor assume
            log("the compositor ended the lock");
            self.quit();
        }
    }

    fn quit(&self) {
        self.done.set(true);
        self.main_loop.quit();
    }

    fn set_sensitive(&self, sensitive: bool) {
        for window in self.windows.borrow().iter() {
            window.entry.set_sensitive(sensitive);
            if sensitive {
                window.entry.grab_focus();
            }
        }
    }

    fn set_status(&self, text: &str, error: bool) {
        for window in self.windows.borrow().iter() {
            window.set_status(text, error);
        }
    }

    fn caps_lock(&self) -> bool {
        self.display.default_seat().and_then(|seat| seat.keyboard()).is_some_and(|keyboard| keyboard.is_caps_locked())
    }

    fn show_caps(&self) {
        if self.busy.get() || self.wait_until.get().is_some() {
            return;
        }
        self.set_status(&if self.caps_lock() { tr("Caps Lock is on") } else { String::new() }, false);
    }

    fn submit(self: &Rc<Self>, entry: &gtk4::PasswordEntry) {
        if self.busy.get() || self.wait_until.get().is_some() || !self.locked.get() {
            return;
        }
        let text = entry.text();
        let mut secret = text.as_bytes().to_vec();
        // A cópia do texto que o GTK devolveu é nossa (g_strdup): zerada antes do g_free
        unsafe { zero(text.as_ptr() as *mut u8, text.len()) };
        drop(text);
        for window in self.windows.borrow().iter() {
            window.entry.set_text("");
        }
        if secret.is_empty() {
            return;
        }
        self.busy.set(true);
        let attempt = self.attempt.get() + 1;
        self.attempt.set(attempt);
        self.set_sensitive(false);
        self.set_status(&tr("Checking…"), false);
        let user = self.user.clone();
        let app = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || match pam::LibPam::open() {
                Ok(lib) => pam::authenticate(&lib, &mut secret, &user, pam::SERVICE),
                Err(err) => {
                    unsafe { zero(secret.as_mut_ptr(), secret.len()) };
                    pam::Outcome { status: pam::Status::Error, code: -1, messages: vec![err] }
                }
            })
            .await
            // Qualquer falha conta como erro, nunca como acerto
            .unwrap_or(pam::Outcome { status: pam::Status::Error, code: -1, messages: Vec::new() });
            if let Some(app) = app.upgrade() {
                app.on_result(result, attempt);
            }
        });
        let app = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(PAM_STUCK_S, move || {
            if let Some(app) = app.upgrade() {
                app.on_stuck(attempt);
            }
        });
    }

    /// O PAM não respondeu (não dá para cancelá-lo): o campo volta, e a resposta velha é ignorada.
    fn on_stuck(&self, attempt: u64) {
        if self.busy.get() && attempt == self.attempt.get() {
            self.attempt.set(attempt + 1);
            self.busy.set(false);
            self.set_status(&tr("The password check did not answer: try again"), true);
            self.set_sensitive(true);
        }
    }

    fn on_result(&self, result: pam::Outcome, attempt: u64) {
        if attempt != self.attempt.get() {
            return; // de uma verificação abandonada: nem um acerto atrasado desbloqueia
        }
        self.busy.set(false);
        let text = match result.status {
            pam::Status::Ok => {
                self.unlocking.set(true);
                self.instance.unlock();
                self.display.sync();
                self.notifier.send(b"U");
                self.quit();
                return;
            }
            pam::Status::Wrong => {
                self.failures.set(self.failures.get() + 1);
                tr("Wrong password")
            }
            pam::Status::Expired => result.messages.last().cloned().unwrap_or_else(|| tr("The account needs attention: log in on a text console")),
            pam::Status::Error => result.messages.last().cloned().unwrap_or_else(|| tr("Could not check the password")),
        };
        for window in self.windows.borrow().iter() {
            window.shake();
        }
        if self.failures.get() >= FAIL_LIMIT {
            self.failures.set(0);
            self.wait_until.set(Some(Instant::now() + FAIL_WAIT));
            self.set_status(&tr("Too many attempts: wait a few seconds"), true);
            return;
        }
        self.set_status(&text, true);
        self.set_sensitive(true);
    }
}

/// Só com o supervisor: sem ele, um bloqueio que cai não teria quem o assumisse.
fn parse_args(args: &[String]) -> Option<i32> {
    match args {
        [flag, fd] if flag == "--notify-fd" && !fd.is_empty() && fd.bytes().all(|b| b.is_ascii_digit()) => fd.parse().ok(),
        _ => None,
    }
}

/// O desfoque de cada monitor ligado, sem GTK: só o GdkPixbuf. Um monitor que falha não impede os outros.
fn blur_outputs() -> ExitCode {
    let Some(outputs) = output_within(Command::new("swaymsg").args(["-r", "-t", "get_outputs"]), Duration::from_secs(5))
        .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out).ok())
    else {
        return ExitCode::SUCCESS;
    };
    let env: HashMap<String, String> = std::env::vars().collect();
    let wallpaper = config::home().join(sl::WALLPAPER);
    for (width, height) in sl::output_sizes(&outputs) {
        if wallpaper.exists() && blur::make_blur(&wallpaper, width, height, &env, BLUR_KEEP).is_none() {
            eprintln!("sway-lock-blur: {width}x{height}: could not make the background");
        }
    }
    ExitCode::SUCCESS
}

/// O desfoque: --blur, ou o binário chamado como sway-lock-blur (o link do --rust-only, ou o exec do
/// script em Python).
fn blur_mode(argv0: &str, args: &[String]) -> Option<Result<(), ()>> {
    let called_as_blur = Path::new(argv0).file_name().is_some_and(|name| name == "sway-lock-blur");
    match (called_as_blur, args) {
        (true, []) => Some(Ok(())),
        (false, [flag]) if flag == "--blur" => Some(Ok(())),
        (true, _) => Some(Err(())),
        _ => None,
    }
}

fn main() -> ExitCode {
    let argv0 = std::env::args().next().unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match blur_mode(&argv0, &args) {
        Some(Ok(())) => return blur_outputs(),
        Some(Err(())) => {
            eprintln!("usage: sway-lock-blur");
            return ExitCode::from(2);
        }
        None => {}
    }
    if args.first().map(String::as_str) == Some("--check") {
        // O install.sh pergunta se esta versão serve aqui: só se o gtk4-layer-shell tem o bloqueio
        return match session_lock::api() {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                log(&err);
                ExitCode::FAILURE
            }
        };
    }
    let Some(fd) = parse_args(&args) else {
        eprintln!("usage: swaydesk-lock --notify-fd N (run it through sway-lock)");
        return ExitCode::from(2);
    };
    // O supervisor vê a queda do locker pelo fim do pipe: os filhos (swaymsg) não podem herdar a ponta de
    // escrita, senão um filho preso adia o fim e a tela fica na cor sólida, sem o swaylock. O Command do
    // Rust não fecha os fds herdados (o subprocess do Python fechava)
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        log(&format!("the notify fd {fd} is not usable: {}", std::io::Error::last_os_error()));
        return ExitCode::FAILURE;
    }
    let api = match session_lock::api() {
        Ok(api) => api,
        Err(err) => {
            log(&err);
            return ExitCode::FAILURE;
        }
    };
    // O cairo, como os popups: uma tela parada, sem animação, e menos memória que o GL
    swaydesk_common::renderer::use_cairo_renderer();
    if let Err(err) = gtk4::init() {
        log(&format!("could not open the display: {err}"));
        return ExitCode::FAILURE;
    }
    if !api.is_supported() {
        log("the compositor has no ext-session-lock-v1");
        return ExitCode::FAILURE;
    }
    let config = config::load();
    swaydesk_common::i18n::init(&config);
    let env: HashMap<String, String> = std::env::vars().collect();
    let mut time_locale = config::effective_locale(&config, &env);
    if !time_locale.is_empty() {
        let name = std::ffi::CString::new(format!("{time_locale}.UTF-8")).unwrap_or_default();
        if unsafe { libc::setlocale(libc::LC_TIME, name.as_ptr()) }.is_null() {
            time_locale.clear();
        }
    }
    let palette = config::palette(&config);
    let display = gdk::Display::default().expect("display");
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&format!("{}{CSS}", config::css_colors(&palette)));
    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let user = sl::current_user().unwrap_or(sl::User { name: String::new(), gecos: String::new(), dir: config::home() });
    let accounts = Path::new(sl::ACCOUNTS);
    let name = sl::real_name(&user, accounts);
    let main_loop = glib::MainLoop::new(None, false);
    let app = Rc::new(App {
        notifier: Notifier(fd),
        main_loop: main_loop.clone(),
        instance: api.instance(),
        display: display.clone(),
        locked: Cell::new(false),
        done: Cell::new(false),
        unlocking: Cell::new(false),
        busy: Cell::new(false),
        attempt: Cell::new(0),
        failures: Cell::new(0),
        wait_until: Cell::new(None),
        windows: RefCell::new(Vec::new()),
        initials: sl::initials(&name),
        name,
        photo_path: sl::photo(&user, accounts),
        user: user.name,
        photo: RefCell::new(None),
        time_locale,
        state: RefCell::new(SystemState::default()),
        art_url: RefCell::new(String::new()),
        art: RefCell::new(None),
    });
    let weak = Rc::downgrade(&app);
    app.instance.connect("monitor", move |values| {
        if let (Some(app), Some(monitor)) = (weak.upgrade(), values.get(1).and_then(|v| v.get::<gdk::Monitor>().ok())) {
            app.add_monitor(&monitor);
        }
    });
    let weak = Rc::downgrade(&app);
    app.instance.connect("locked", move |_| {
        if let Some(app) = weak.upgrade() {
            app.on_locked();
        }
    });
    let weak = Rc::downgrade(&app);
    app.instance.connect("failed", move |_| {
        if let Some(app) = weak.upgrade() {
            app.on_failed();
        }
    });
    let weak = Rc::downgrade(&app);
    app.instance.connect("unlocked", move |_| {
        if let Some(app) = weak.upgrade() {
            app.on_unlocked();
        }
    });
    if let Some(keyboard) = display.default_seat().and_then(|seat| seat.keyboard()) {
        let weak = Rc::downgrade(&app);
        keyboard.connect_caps_lock_state_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                app.show_caps();
            }
        });
    }
    let weak = Rc::downgrade(&app);
    glib::timeout_add_seconds_local(1, move || match weak.upgrade() {
        Some(app) => {
            app.tick();
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    if !app.instance.lock() && !app.done.get() {
        // Sem o ::failed: sem "L" nem "F", o sway-lock tenta o próximo
        log("could not start the lock");
        return ExitCode::FAILURE;
    }
    if !app.done.get() {
        main_loop.run();
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os filhos (swaymsg) abertos de uma linha com sinais bloqueados (as do GIO bloqueiam todos) nascem
    /// com a máscara limpa, pelo [`unblocked`]: o Command da biblioteca padrão não a zera, e um swaymsg
    /// que herdasse o SIGTERM bloqueado não sairia com um pkill e ficaria órfão. O grep direto, sem um sh
    /// no meio (o dash limpa a máscara ao subir, e o teste passaria sempre).
    #[test]
    fn children_start_with_no_blocked_signals() {
        let blocked = std::thread::spawn(|| unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut set);
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            output_within(Command::new("grep").args(["SigBlk", "/proc/self/status"]), Duration::from_secs(5))
        })
        .join()
        .unwrap()
        .expect("sh");
        let text = String::from_utf8_lossy(&blocked);
        assert!(text.trim_end().ends_with("0000000000000000"), "{text}");
    }

    #[test]
    fn blur_by_flag_or_by_name() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(blur_mode("/home/a/.local/bin/swaydesk-lock", &args(&["--blur"])), Some(Ok(())));
        assert_eq!(blur_mode("sway-lock-blur", &args(&[])), Some(Ok(())));
        assert_eq!(blur_mode("/x/sway-lock-blur", &args(&["extra"])), Some(Err(())));
        assert_eq!(blur_mode("swaydesk-lock", &args(&["--notify-fd", "5"])), None);
        assert_eq!(blur_mode("swaydesk-lock", &args(&[])), None);
    }

    #[test]
    fn only_with_the_supervisor() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&["--notify-fd", "5"])), Some(5));
        assert_eq!(parse_args(&args(&[])), None);
        assert_eq!(parse_args(&args(&["--notify-fd", "-1"])), None);
        assert_eq!(parse_args(&args(&["--notify-fd", "5", "x"])), None);
    }
}

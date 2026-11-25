//! Indicador de volume e brilho na tela: ícone, barra e porcentagem, embaixo e acima da dock. O porte do
//! `sway-osd-bar` em Python.
//!
//! Fica aberto a sessão toda, escondido. O sway-osd muda o nível, grava o estado em
//! $XDG_RUNTIME_DIR/sway-osd-bar.state e manda SIGUSR1 ao PID de sway-osd-bar.pid. Ao receber, este
//! programa lê o estado, aparece por SHOW_MS e se esconde de novo. Escondida, a janela some de vez: na
//! próxima vez abre na tela com o foco. Lê as cores ao abrir: a troca de tema o encerra, e o laço do
//! config do Sway o abre de novo. --check: só confere que o binário carrega.
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use swaydesk_common::resident;

const PROGRAM: &str = "sway-osd-bar";
const SHOW: Duration = Duration::from_millis(1000);
/// Acima da dock, onde ficava o wob
const MARGIN_BOTTOM: i32 = 90;
/// Nerd Font: os do ícone do volume (fa-volume-off, -down, -up) e um alto-falante cortado para o mudo
const VOLUME_ICONS: [&str; 3] = ["\u{f026}", "\u{f027}", "\u{f028}"];
const MUTED_ICON: &str = "\u{f075f}";
/// Os sóis do ícone do brilho (md-brightness-5, -6, -7)
const BRIGHTNESS_ICONS: [&str; 3] = ["\u{f00df}", "\u{f00e0}", "\u{f00e1}"];

const CSS: &str = r#"
window { background: transparent; }
.osd {
  background: rgba(46, 46, 51, 0.97);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 10px 16px;
  color: #dadada;
}
.osd-icon { font-family: "Symbols Nerd Font"; font-size: 18px; min-width: 24px; }
.osd-value { font-size: 13px; font-feature-settings: "tnum"; }
.osd progressbar trough {
  min-height: 6px; min-width: 200px; border-radius: 3px; background: #26262a; border: none;
}
/* O Adwaita do GTK 4 dá margin: 0 -1px ao progress: a largura mínima saía -2, com um aviso do GTK */
.osd progressbar progress { min-height: 6px; border-radius: 3px; border: none; margin: 0; background: @accent; }
.osd.muted progressbar progress { background: #8a8a8f; }
.osd.muted .osd-icon, .osd.muted .osd-value { color: #8a8a8f; }
.osd.brightness progressbar progress { background: #ffffff; }
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Volume,
    Brightness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct State {
    kind: Kind,
    level: u32,
    muted: bool,
}

/// A linha do sway-osd: "volume 70", "volume 70 muted" ou "brightness 40". O volume pode passar de 100;
/// o nível nunca é negativo. None se não fizer sentido.
fn parse_state(text: &str) -> Option<State> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let kind = match parts.first()? {
        &"volume" => Kind::Volume,
        &"brightness" => Kind::Brightness,
        _ => return None,
    };
    // O int() do Python aceita o sinal: "-5" é recusado por ser negativo, "+5" vale 5
    let number = parts.get(1)?;
    let level = number.strip_prefix('+').unwrap_or(number).parse::<u32>().ok()?;
    let muted = kind == Kind::Volume && parts[2..].contains(&"muted");
    Some(State { kind, level, muted })
}

/// (ícone, classe do CSS, fração da barra entre 0 e 1). Os mesmos limiares dos ícones da barra.
fn look(state: &State) -> (&'static str, &'static str, f64) {
    let icons = if state.kind == Kind::Volume { VOLUME_ICONS } else { BRIGHTNESS_ICONS };
    let icon = if state.level < 34 { icons[0] } else if state.level < 67 { icons[1] } else { icons[2] };
    let fraction = f64::from(state.level.min(100)) / 100.0;
    if state.muted {
        (MUTED_ICON, "muted", fraction)
    } else {
        (icon, if state.kind == Kind::Volume { "volume" } else { "brightness" }, fraction)
    }
}

fn run_dir() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/tmp".into()))
}

fn read_state(path: &Path) -> Option<State> {
    parse_state(&std::fs::read_to_string(path).ok()?)
}

fn write_pidfile(path: &Path) -> std::io::Result<()> {
    let tmp = PathBuf::from(format!("{}.{}", path.display(), std::process::id()));
    std::fs::write(&tmp, format!("{}\n", std::process::id()))?;
    std::fs::rename(&tmp, path)
}

/// Só se ainda for o nosso: um sway-osd-bar novo pode já ter gravado o dele.
fn remove_pidfile(path: &Path) {
    let mine = std::fs::read_to_string(path).ok().and_then(|t| t.trim().parse::<u32>().ok()) == Some(std::process::id());
    if mine {
        let _ = std::fs::remove_file(path);
    }
}

/// O arquivo da trava, aberto e travado, ou None se outro sway-osd-bar já a tem. Dura enquanto o arquivo
/// ficar aberto: duas instâncias que sobem juntas nunca ficam as duas.
fn single_instance(path: &Path) -> Option<File> {
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600)
        .custom_flags(libc::O_NOFOLLOW).open(path).ok()?;
    // SAFETY: o flock recebe um descritor aberto, que vive enquanto o File viver
    (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(file)
}

struct Osd {
    window: gtk4::Window,
    card: gtk4::Box,
    icon: gtk4::Label,
    bar: gtk4::ProgressBar,
    value: gtk4::Label,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Osd {
    fn new() -> Rc<Osd> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&format!("{}{CSS}", swaydesk_ui::theme_css()));
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)] // sem substituto no GTK 4.18
            gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let window = gtk4::Window::new();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Bottom, true);
        window.set_margin(Edge::Bottom, MARGIN_BOTTOM);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_namespace(PROGRAM);
        let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        card.add_css_class("osd");
        let icon = gtk4::Label::new(None);
        icon.add_css_class("osd-icon");
        let bar = gtk4::ProgressBar::new();
        bar.set_valign(gtk4::Align::Center);
        bar.set_hexpand(true);
        // "100%" cabe; com o número alinhado à direita, o cartão não muda de largura
        let value = gtk4::Label::new(None);
        value.set_xalign(1.0);
        value.set_width_chars(4);
        value.add_css_class("osd-value");
        card.append(&icon);
        card.append(&bar);
        card.append(&value);
        window.set_child(Some(&card));
        // Os cliques passam direto para o que estiver embaixo. A cada vez que aparece: a superfície do
        // layer-shell é refeita a cada show, e o GTK não guarda a região de antes
        window.connect_map(|window| {
            if let Some(surface) = window.surface() {
                surface.set_input_region(&gtk4::cairo::Region::create());
            }
        });
        Rc::new(Osd { window, card, icon, bar, value, timer: RefCell::default() })
    }

    fn show_state(self: &Rc<Self>, state: &State) {
        let (icon, css, fraction) = look(state);
        for name in ["volume", "brightness", "muted"] {
            if name == css {
                self.card.add_css_class(name);
            } else {
                self.card.remove_css_class(name);
            }
        }
        self.icon.set_text(icon);
        self.bar.set_fraction(fraction);
        self.value.set_text(&format!("{}%", state.level));
        self.window.set_visible(true);
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        let me = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(SHOW, move || {
            if let Some(osd) = me.upgrade() {
                osd.timer.replace(None);
                osd.window.set_visible(false);
            }
        });
        self.timer.replace(Some(timer));
    }
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-osd-bar\n",
    "    The on-screen volume and brightness indicator (sway-osd shows it).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().skip(1).any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    // Aberto como swaydesk-osd-bar (o PATH, um `exec` no config de um Sway puro), o sway-osd não o acharia
    resident::run_as(PROGRAM);
    resident::set_process_name(PROGRAM);
    let run = run_dir();
    // Com outro no ar, o 4: a unit do systemd não o reabre ao lado dele
    let Some(_lock) = single_instance(&run.join(format!("{PROGRAM}.lock"))) else {
        return resident::another_instance("swaydesk-osd-bar", PROGRAM, Some(&run.join(format!("{PROGRAM}.pid"))));
    };
    swaydesk_ui::use_cairo_renderer(); // residente: a menor memória; o cartão aparece por 1 s, sem animação
    swaydesk_common::theme_resources::register_theme_resources();
    if let Err(err) = gtk4::init() {
        eprintln!("{PROGRAM}: could not start GTK: {err}");
        return ExitCode::FAILURE;
    }
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }
    let osd = Osd::new();
    let main_loop = glib::MainLoop::new(None, false);
    // Os sinais antes do PID: um SIGUSR1 sem o tratamento encerraria o programa
    let state_file = run.join(format!("{PROGRAM}.state"));
    let shown = osd.clone();
    glib::unix_signal_add_local(libc::SIGUSR1, move || {
        if let Some(state) = read_state(&state_file) {
            shown.show_state(&state);
        }
        glib::ControlFlow::Continue
    });
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }
    let pidfile = run.join(format!("{PROGRAM}.pid"));
    if let Err(err) = write_pidfile(&pidfile) {
        eprintln!("{PROGRAM}: could not write {}: {err}", pidfile.display());
    }
    main_loop.run();
    remove_pidfile(&pidfile);
    let _ = std::io::stderr().flush();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(kind: Kind, level: u32, muted: bool) -> Option<State> {
        Some(State { kind, level, muted })
    }

    #[test]
    fn parse() {
        assert_eq!(parse_state("volume 70\n"), state(Kind::Volume, 70, false));
        assert_eq!(parse_state("volume 0 muted\n"), state(Kind::Volume, 0, true));
        assert_eq!(parse_state("brightness 40\n"), state(Kind::Brightness, 40, false));
        assert_eq!(parse_state("volume 150"), state(Kind::Volume, 150, false));
        assert_eq!(parse_state("brightness 40 muted"), state(Kind::Brightness, 40, false));
        for text in ["", "volume", "volume x", "volume -5", "mic 30", "brightness 1.5"] {
            assert_eq!(parse_state(text), None, "{text}");
        }
    }

    #[test]
    fn look_follows_the_bar_icons() {
        let s = |kind, level, muted| State { kind, level, muted };
        assert_eq!(look(&s(Kind::Volume, 33, false)), (VOLUME_ICONS[0], "volume", 0.33));
        assert_eq!(look(&s(Kind::Volume, 34, false)).0, VOLUME_ICONS[1]);
        assert_eq!(look(&s(Kind::Volume, 67, false)).0, VOLUME_ICONS[2]);
        assert_eq!(look(&s(Kind::Volume, 80, true)), (MUTED_ICON, "muted", 0.8));
        assert_eq!(look(&s(Kind::Brightness, 100, false)), (BRIGHTNESS_ICONS[2], "brightness", 1.0));
        assert_eq!(look(&s(Kind::Volume, 150, false)).2, 1.0);
    }

    #[test]
    fn pidfile_and_lock() {
        let dir = std::env::temp_dir().join(format!("swaydesk-osd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid = dir.join("bar.pid");
        write_pidfile(&pid).unwrap();
        std::fs::write(&pid, "1\n").unwrap();
        remove_pidfile(&pid);
        assert!(pid.exists());
        write_pidfile(&pid).unwrap();
        remove_pidfile(&pid);
        assert!(!pid.exists());
        let lock = dir.join("bar.lock");
        let first = single_instance(&lock);
        assert!(first.is_some());
        assert!(single_instance(&lock).is_none()); // um open novo não herda a trava
        drop(first);
        assert!(single_instance(&lock).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! Painéis de captura de tela e de gravação de tela, no estilo dos popups da Waybar. O porte do
//! `sway-capture` em Python.
//!
//! uso: sway-capture shot   captura de tela
//!      sway-capture rec    gravação de tela; gravando, para
//!
//! Abre no centro da tela sobre um fundo escurecido (o [`ModalPanel`]). Clicar fora ou Esc fecha; o atalho
//! do mesmo painel também fecha, e o do outro troca de painel. As teclas são iniciais em inglês: r Region ·
//! s Screen · w Window, g Gradia (só com ele instalado), d Delay, a Audio; na gravação, Shift+tecla grava o
//! som do sistema e Ctrl+tecla o microfone. Quem captura e grava continuam sendo o sway-shot e o sway-rec; o
//! painel só escolhe as opções e os chama depois de fechar (para não sair na imagem). As últimas escolhas
//! (Gradia e áudio) ficam em ~/.cache/sway-capture.json; o atraso vale só para aquela captura.
//! --check: só confere que o binário carrega.
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use swaydesk_common::config;
use swaydesk_common::i18n::tr;
use swaydesk_ui::cmd::{bin, spawn_detached};
use swaydesk_ui::label;
use swaydesk_ui::modal::ModalPanel;

const PROGRAM: &str = "sway-capture";
const GRADIA_ID: &str = "be.alexandervanhee.gradia";

// Material da Nerd Font
const I_REGION: &str = "\u{f0489}";
const I_SCREEN: &str = "\u{f0379}";
const I_WINDOW: &str = "\u{f0614}";
const I_CAMERA: &str = "\u{f0100}";
const I_RECORD: &str = "\u{f044a}";
const I_EDIT: &str = "\u{f11e3}";
const I_TIMER: &str = "\u{f051b}";
const I_FOLDER: &str = "\u{f024b}";
const I_MUTE: &str = "\u{f0581}";
const I_SPEAKER: &str = "\u{f057e}";
const I_MIC: &str = "\u{f036c}";

/// (chave, texto, ícone, tecla): a tecla é a inicial em inglês
const TARGETS: [(&str, &str, &str, &str); 3] =
    [("region", "Region", I_REGION, "r"), ("full", "Full screen", I_SCREEN, "s"), ("window", "Window", I_WINDOW, "w")];
const DELAYS: [u32; 4] = [0, 3, 5, 10];
const AUDIO: [(&str, &str, &str); 3] = [("none", "No audio", I_MUTE), ("system", "System", I_SPEAKER), ("mic", "Microphone", I_MIC)];

const CSS: &str = r#"
window { background: rgba(0, 0, 0, 0.35); }
.card {
  margin: 24px;  /* room for the box-shadow (otherwise clipped to a square) */
  background: rgba(46, 46, 51, 0.98);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 16px 18px;
  color: #dadada;
  box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5);
}
.head-icon { font-family: "Symbols Nerd Font"; font-size: 22px; color: @accent; }
.head-icon.rec { color: #fc4138; }
.title  { font-size: 16px; font-weight: bold; color: #ffffff; }
.section { color: #8a8a8f; font-size: 12px; font-weight: bold; }
.hint   { color: #8a8a8f; font-size: 11px; }
button.target {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  border-radius: 10px; padding: 12px 6px; min-width: 104px;
}
button.target:hover { background: #4c4c50; }
button.target:focus-visible, button.target:focus { outline: none; box-shadow: inset 0 0 0 2px @accent; }
.t-icon { font-family: "Symbols Nerd Font"; font-size: 28px; }
.t-name { font-size: 13px; }
.key {
  font-size: 10px; color: #8a8a8f; background: #2e2e33;
  border-radius: 4px; padding: 0 5px;
}
button.seg {
  background: #38383d; border: none; box-shadow: none; color: #dadada;
  padding: 6px 4px; border-radius: 8px; min-height: 0; font-size: 13px;
}
button.seg:hover { background: #4c4c50; }
button.seg.active { background: @accent; color: @on_accent; }
button.seg.active:hover { background: @accent_hover; }
.seg-icon { font-family: "Symbols Nerd Font"; font-size: 14px; }
switch { background: #38383d; border: none; border-radius: 12px; min-width: 52px; min-height: 24px; }
switch:checked { background: @accent; }
switch slider { background: #8a8a8f; border-radius: 12px; border: none; box-shadow: none; min-width: 26px; min-height: 24px; margin: 0; }
switch:checked slider { background: #ffffff; }
switch image { color: transparent; }
.row-icon { font-family: "Symbols Nerd Font"; font-size: 16px; }
.link-icon { font-family: "Symbols Nerd Font"; font-size: 14px; }
button.link {
  background: transparent; border: none; box-shadow: none; color: #8a8a8f;
  padding: 0 4px; min-height: 0; font-size: 12px;
}
button.link:hover { color: #ffffff; }
button.link > box > label { text-decoration-line: none; }
"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Shot,
    Rec,
}

/// Gradia instalado? A pasta do flatpak (do sistema ou do usuário), instantânea, ou um `gradia` no PATH.
fn gradia_installed() -> bool {
    let user = config::data_home().join("flatpak/app").join(GRADIA_ID);
    PathBuf::from("/var/lib/flatpak/app").join(GRADIA_ID).is_dir() || user.is_dir() || glib::find_program_in_path("gradia").is_some()
}

fn prefs_path() -> PathBuf {
    config::cache_home().join("sway-capture.json")
}

/// As escolhas lembradas: só valores conhecidos, para um arquivo editado ou corrompido não impedir o painel.
#[derive(Clone, Debug, Default, PartialEq)]
struct Prefs {
    audio: Option<String>,
    gradia: Option<bool>,
}

fn parse_prefs(text: &str) -> Prefs {
    let Ok(serde_json::Value::Object(data)) = serde_json::from_str::<serde_json::Value>(text) else { return Prefs::default() };
    Prefs {
        audio: data.get("audio").and_then(|a| a.as_str()).filter(|a| AUDIO.iter().any(|(k, ..)| k == a)).map(str::to_string),
        gradia: data.get("gradia").and_then(serde_json::Value::as_bool),
    }
}

fn load_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path()).map(|t| parse_prefs(&t)).unwrap_or_default()
}

fn save_prefs(prefs: &Prefs) {
    let mut data = serde_json::Map::new();
    if let Some(audio) = &prefs.audio {
        data.insert("audio".into(), audio.clone().into());
    }
    if let Some(gradia) = prefs.gradia {
        data.insert("gradia".into(), gradia.into());
    }
    // Numa conta nova, o ~/.cache pode não existir ainda
    let _ = std::fs::create_dir_all(config::cache_home());
    let _ = std::fs::write(prefs_path(), serde_json::Value::Object(data).to_string());
}

/// A pasta de imagens ou de vídeos (xdg-user-dir), com a subpasta da captura.
fn user_dir(kind: &str, fallback: &str, sub: &str) -> PathBuf {
    let found = Command::new("xdg-user-dir").arg(kind).stderr(Stdio::null()).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|d| !d.is_empty());
    found.map(PathBuf::from).unwrap_or_else(|| config::home().join(fallback)).join(sub)
}

/// Espera e depois executa. O comando vai como argumentos, não como texto para o shell: o caminho pode ter
/// espaços. O sh sai deste processo, então o painel já fechou quando a captura acontece.
fn spawn_after(seconds: f64, argv: &[String]) {
    let mut args = vec!["-c".to_string(), r#"sleep "$1"; shift; exec "$@""#.to_string(), "sh".to_string(), format_seconds(seconds)];
    args.extend(argv.iter().cloned());
    if let Err(err) = spawn_detached("sh", &args) {
        eprintln!("{PROGRAM}: could not run {}: {err}", argv.first().map(String::as_str).unwrap_or(""));
    }
}

/// O `f"{s:g}"` do Python: 0.25, 3.25, 10.25.
fn format_seconds(seconds: f64) -> String {
    let text = format!("{seconds:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Botões lado a lado com o escolhido na cor de destaque.
struct Segmented {
    root: gtk4::Box,
    buttons: Vec<(String, gtk4::Button)>,
    value: RefCell<String>,
    on_change: Box<dyn Fn(&str)>,
}

impl Segmented {
    fn new(items: &[(String, String, Option<&str>)], current: &str, on_change: impl Fn(&str) + 'static) -> Rc<Segmented> {
        let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        root.set_homogeneous(true);
        let mut buttons = Vec::new();
        for (key, text, icon) in items {
            let button = gtk4::Button::new();
            button.add_css_class("seg");
            let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            content.set_halign(gtk4::Align::Center);
            if let Some(icon) = icon {
                content.append(&label(icon, &["seg-icon"], 0.5));
            }
            content.append(&label(text, &[], 0.5));
            button.set_child(Some(&content));
            root.append(&button);
            buttons.push((key.clone(), button));
        }
        let seg = Rc::new(Segmented { root, buttons, value: RefCell::new(String::new()), on_change: Box::new(on_change) });
        for (key, button) in &seg.buttons {
            let (weak, key) = (Rc::downgrade(&seg), key.clone());
            button.connect_clicked(move |_| {
                if let Some(s) = weak.upgrade() {
                    s.set(&key, true);
                }
            });
        }
        seg.set(current, false);
        seg
    }

    fn set(&self, key: &str, notify: bool) {
        self.value.replace(key.to_string());
        for (k, button) in &self.buttons {
            if k == key {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
        if notify {
            (self.on_change)(key);
        }
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// A próxima opção, na ordem, voltando ao começo.
    fn cycle(&self) {
        let current = self.value();
        let index = self.buttons.iter().position(|(k, _)| *k == current).unwrap_or(0);
        let next = self.buttons[(index + 1) % self.buttons.len()].0.clone();
        self.set(&next, true);
    }
}

struct Panel {
    kind: Kind,
    panel: Rc<ModalPanel>,
    prefs: RefCell<Prefs>,
    target_buttons: Vec<gtk4::Button>,
    gradia: RefCell<Option<gtk4::Switch>>,
    delay: RefCell<Option<Rc<Segmented>>>,
    audio: RefCell<Option<Rc<Segmented>>>,
    acted: Cell<bool>,
}

impl Panel {
    fn new(kind: Kind, main_loop: &glib::MainLoop) -> Rc<Panel> {
        let panel = ModalPanel::new(PROGRAM, PROGRAM, CSS, 12, main_loop);
        let card = panel.card.clone();

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        if kind == Kind::Shot {
            head.append(&label(I_CAMERA, &["head-icon"], 0.5));
            head.append(&label(&tr("Screenshot"), &["title"], 0.0));
        } else {
            head.append(&label(I_RECORD, &["head-icon", "rec"], 0.5));
            head.append(&label(&tr("Record screen"), &["title"], 0.0));
        }
        card.append(&head);

        // Alvo: região, tela inteira, janela
        let targets = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        targets.set_homogeneous(true);
        let mut target_buttons = Vec::new();
        for (_, name, icon, letter) in TARGETS {
            let button = gtk4::Button::new();
            button.add_css_class("target");
            let column = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            column.append(&label(icon, &["t-icon"], 0.5));
            column.append(&label(&tr(name), &["t-name"], 0.5));
            let key = label(letter, &["key"], 0.5);
            key.set_halign(gtk4::Align::Center);
            column.append(&key);
            button.set_child(Some(&column));
            targets.append(&button);
            target_buttons.push(button);
        }
        card.append(&targets);

        let me = Rc::new(Panel {
            kind,
            panel,
            prefs: RefCell::new(load_prefs()),
            target_buttons,
            gradia: RefCell::default(),
            delay: RefCell::default(),
            audio: RefCell::default(),
            acted: Cell::new(false),
        });
        for (button, (target, ..)) in me.target_buttons.iter().zip(TARGETS) {
            let weak = Rc::downgrade(&me);
            button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.go(target, None);
                }
            });
        }
        if kind == Kind::Shot {
            me.build_shot(&card);
        } else {
            me.build_rec(&card);
        }

        // Rodapé: dica das teclas e a pasta
        let foot = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let hint = if kind == Kind::Shot { tr("Esc closes") } else { tr("Shift + key: system · Ctrl + key: microphone · Esc closes") };
        foot.append(&label(&hint, &["hint"], 0.0));
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        foot.append(&spacer);
        let folder = gtk4::Button::new();
        folder.add_css_class("link");
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8); // o glifo da pasta não tem folga à direita
        content.append(&label(I_FOLDER, &["link-icon"], 0.5));
        content.append(&label(&tr("Open folder"), &[], 0.0));
        folder.set_child(Some(&content));
        let weak = Rc::downgrade(&me);
        folder.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.open_folder();
            }
        });
        foot.append(&folder);
        card.append(&foot);

        let keys = gtk4::EventControllerKey::new();
        let weak = Rc::downgrade(&me);
        keys.connect_key_pressed(move |_, key, _, state| match weak.upgrade() {
            Some(p) => p.on_key(key, state),
            None => glib::Propagation::Proceed,
        });
        me.panel.window.add_controller(keys);
        me.panel.show();
        me.target_buttons[0].grab_focus();
        me
    }

    fn build_shot(self: &Rc<Self>, card: &gtk4::Box) {
        // Sem o Gradia, a opção some e a tecla g não age
        if gradia_installed() {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
            row.append(&label(I_EDIT, &["row-icon"], 0.5));
            let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            column.append(&label(&tr("Open in Gradia"), &[], 0.0));
            column.append(&label(&tr("to annotate, crop and decorate · g"), &["hint"], 0.0));
            column.set_hexpand(true);
            row.append(&column);
            let switch = gtk4::Switch::new();
            switch.set_valign(gtk4::Align::Center);
            switch.set_active(self.prefs.borrow().gradia.unwrap_or(true));
            let weak = Rc::downgrade(self);
            switch.connect_active_notify(move |s| {
                if let Some(p) = weak.upgrade() {
                    p.prefs.borrow_mut().gradia = Some(s.is_active());
                    save_prefs(&p.prefs.borrow());
                }
            });
            row.append(&switch);
            card.append(&row);
            self.gradia.replace(Some(switch));
        }
        card.append(&label(&format!("{I_TIMER}  {}", tr("Delay · d")), &["section"], 0.0));
        let items: Vec<(String, String, Option<&str>)> =
            DELAYS.iter().map(|d| (d.to_string(), if *d == 0 { tr("Now") } else { format!("{d} s") }, None)).collect();
        let delay = Segmented::new(&items, "0", |_| {});
        card.append(&delay.root);
        self.delay.replace(Some(delay));
    }

    fn build_rec(self: &Rc<Self>, card: &gtk4::Box) {
        card.append(&label(&tr("Audio · a"), &["section"], 0.0));
        let items: Vec<(String, String, Option<&str>)> = AUDIO.iter().map(|(k, t, i)| (k.to_string(), tr(t), Some(*i))).collect();
        let current = self.prefs.borrow().audio.clone().unwrap_or_else(|| "none".into());
        let weak = Rc::downgrade(self);
        let audio = Segmented::new(&items, &current, move |value| {
            if let Some(p) = weak.upgrade() {
                p.prefs.borrow_mut().audio = Some(value.to_string());
                save_prefs(&p.prefs.borrow());
            }
        });
        card.append(&audio.root);
        self.audio.replace(Some(audio));
    }

    fn go(&self, target: &str, audio: Option<&str>) {
        if self.acted.replace(true) || self.panel.is_closing() {
            return; // uma tecla e um clique juntos não capturam duas vezes
        }
        if self.kind == Kind::Shot {
            let delay: f64 = self.delay.borrow().as_ref().and_then(|d| d.value().parse().ok()).unwrap_or(0.0);
            let mut argv = vec![bin("sway-shot").to_string_lossy().into_owned(), target.to_string()];
            if self.gradia.borrow().as_ref().is_some_and(|s| s.is_active()) {
                argv.push("edit".into());
            }
            // espera o painel sumir da tela (e o atraso escolhido) antes de capturar
            spawn_after(0.25 + delay, &argv);
        } else {
            let audio = audio.map(str::to_string).or_else(|| self.audio.borrow().as_ref().map(|a| a.value())).unwrap_or_else(|| "none".into());
            spawn_after(0.25, &[bin("sway-rec").to_string_lossy().into_owned(), target.to_string(), audio]);
        }
        self.panel.close(target);
    }

    fn open_folder(&self) {
        let folder = if self.kind == Kind::Shot {
            user_dir("PICTURES", "Pictures", "Screenshots")
        } else {
            user_dir("VIDEOS", "Videos", "Screencasts")
        };
        let opened = std::fs::create_dir_all(&folder).and_then(|_| spawn_detached("xdg-open", &[&folder]));
        if opened.is_err() {
            let message = tr("Could not open %s").replacen("%s", &folder.display().to_string(), 1);
            let _ = spawn_detached("notify-send", &["-u".to_string(), "critical".into(), tr("Capture"), message]);
        }
        self.panel.close("pasta");
    }

    fn on_key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let name = key.to_lower().name().map(|n| n.to_lowercase()).unwrap_or_default();
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        // Esc: o ModalPanel. Alt e Super não fazem parte de nenhum atalho do painel
        if state.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK) {
            return glib::Propagation::Proceed;
        }
        if let Some((target, ..)) = TARGETS.iter().find(|(_, _, _, letter)| *letter == name) {
            if self.kind == Kind::Rec && (shift || ctrl) {
                self.go(target, Some(if ctrl { "mic" } else { "system" }));
            } else {
                self.go(target, None);
            }
            return glib::Propagation::Stop;
        }
        match (self.kind, name.as_str()) {
            (Kind::Shot, "g") => {
                if let Some(switch) = self.gradia.borrow().as_ref() {
                    switch.set_active(!switch.is_active());
                    return glib::Propagation::Stop;
                }
            }
            (Kind::Shot, "d") => {
                if let Some(delay) = self.delay.borrow().as_ref() {
                    delay.cycle();
                    return glib::Propagation::Stop;
                }
            }
            (Kind::Rec, "a") => {
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.cycle();
                    return glib::Propagation::Stop;
                }
            }
            _ => {}
        }
        glib::Propagation::Proceed // Tab, setas, Enter e espaço navegam e ativam os botões
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--check") {
        return ExitCode::SUCCESS;
    }
    let kind = match args.first().map(String::as_str) {
        Some("shot") => Kind::Shot,
        Some("rec") => Kind::Rec,
        _ => {
            eprintln!("{}", tr("usage: sway-capture shot|rec"));
            return ExitCode::from(2);
        }
    };
    // Gravando: o atalho de gravar para a gravação, como antes
    if kind == Kind::Rec && Command::new("pgrep").args(["-u", &swaydesk_common::resident::own_uid(), "-x", "wf-recorder"]).stdout(Stdio::null()).status().is_ok_and(|s| s.success()) {
        let _ = Command::new(bin("sway-rec")).arg("stop").status();
        return ExitCode::SUCCESS;
    }
    let name = args[0].clone();
    // O atalho do mesmo painel fecha; o do outro painel troca de um para o outro
    swaydesk_ui::run_single(PROGRAM, &[], move |open| open.first() == Some(&name), move |main_loop| Some(Panel::new(kind, main_loop)), || {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_keep_only_known_values() {
        assert_eq!(parse_prefs(r#"{"audio": "mic", "gradia": false}"#), Prefs { audio: Some("mic".into()), gradia: Some(false) });
        assert_eq!(parse_prefs(r#"{"audio": "loud", "gradia": "yes"}"#), Prefs::default());
        assert_eq!(parse_prefs("[1, 2]"), Prefs::default());
        assert_eq!(parse_prefs("not json"), Prefs::default());
    }

    #[test]
    fn seconds_like_python_g() {
        assert_eq!(format_seconds(0.25), "0.25");
        assert_eq!(format_seconds(3.25), "3.25");
        assert_eq!(format_seconds(10.0), "10");
    }

    #[test]
    fn same_texts_and_keys_as_python() {
        let Some(path) = swaydesk_common::reference::repo_file(env!("CARGO_MANIFEST_DIR"), "home/.local/bin/sway-capture") else { return };
        let python = std::fs::read_to_string(path).unwrap();
        for (key, text, _, letter) in TARGETS {
            assert!(python.contains(&format!("(\"{key}\", _(\"{text}\"), ")), "{key}");
            assert!(python.contains(&format!(", \"{letter}\")")), "{letter}");
        }
        for (key, text, _) in AUDIO {
            assert!(python.contains(&format!("(\"{key}\", _(\"{text}\"), ")), "{key}");
        }
        assert!(python.contains("DELAYS = [0, 3, 5, 10]"));
        assert!(python.contains(r#"'sleep "$1"; shift; exec "$@"'"#));
    }
}

//! Menu da sessão no estilo dos popups da Waybar: o porte de `home/.local/bin/session-menu`, que passa a
//! vez a este binário (sway_rust).
//!
//! Abre no centro da tela sobre um fundo escurecido (layer-shell overlay, teclado exclusivo): Bloquear ·
//! Sair · Suspender · Reiniciar · Desligar. As teclas são as iniciais em inglês (Lock · Exit · Suspend ·
//! Reboot · Power off: l · e · s · r · p), para não depender do idioma da interface. Setas e Tab navegam,
//! Enter ativa; clicar fora ou Esc fecha. As ações passam pelo session-action, que sabe o que fazer em
//! cada sessão (Sway, Cinnamon, GNOME). Chamado por Ctrl+Alt+Delete e pelo botão de energia da Waybar.
use std::ffi::CStr;
use std::process::ExitCode;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use swaydesk_common::i18n::{ntr, tr};
use swaydesk_common::resident;
use swaydesk_ui::cmd::{bin, spawn_detached};
use swaydesk_ui::label;
use swaydesk_ui::modal::ModalPanel;

const PROGRAM: &str = "session-menu";

/// Roda a ação à parte, depois de o menu fechar, e avisa se ela falhar: com o menu já fechado, um
/// bloqueio que não aconteceu passaria despercebido. A saída vai para um arquivo, não para um pipe: o
/// bloqueio fica em segundo plano e prenderia o pipe. $0 = session-action, $1 = ação, $2 = nome da ação,
/// $3 = log, $4 = título do aviso, $5 = texto do aviso com %s no lugar do nome. O mesmo texto do Python.
const RUNNER: &str = r#"sleep 0.2
if ! "$0" "$1" >"$3" 2>&1; then
  notify-send -u critical "$4" "$(printf "$5" "$2") $(tail -n 1 "$3")"
fi
"#;

/// (ação do session-action, texto, ícone Material da Nerd Font, tecla, classe). Tecla = inicial do
/// nome em inglês: Lock, Exit, Suspend, Reboot, Power off.
const ACTIONS: [(&str, &str, &str, &str, &str); 5] = [
    ("lock", "Lock", "\u{f033e}", "l", ""),
    ("logout", "Log out", "\u{f0343}", "e", ""),
    ("suspend", "Suspend", "\u{f0904}", "s", ""),
    ("reboot", "Restart", "\u{f0709}", "r", "warn"),
    ("poweroff", "Shut down", "\u{f0425}", "p", "danger"),
];
const I_USER: &str = "\u{f0006}";

// O do Python, trocando o box-shadow do GTK 3 pelo do GTK 4 (igual) e tirando o padrão do Adwaita
const CSS: &str = r#"
window { background: rgba(0, 0, 0, 0.45); }
.card {
  margin: 24px;
  background: rgba(46, 46, 51, 0.98);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  padding: 18px 20px;
  color: #dadada;
  box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5);
}
.head-icon { font-family: "Symbols Nerd Font"; font-size: 30px; color: @accent; }
.title  { font-size: 16px; font-weight: bold; color: #ffffff; }
.sub    { color: #8a8a8f; font-size: 12px; }
.hint   { color: #8a8a8f; font-size: 11px; }
button.act {
  background: #38383d; border: none; box-shadow: none; color: #dadada; outline: none;
  border-radius: 10px; padding: 16px 8px 12px 8px; min-width: 104px;
}
/* Só o botão com o foco tem cor: é o que o Enter ativa */
button.act:hover { background: #4c4c50; color: #ffffff; }
button.act:focus { background: @accent; color: @on_accent; }
button.act.warn:focus { background: #f27835; color: #ffffff; }
button.act.danger:focus { background: #fc4138; color: #ffffff; }
button.act:focus { box-shadow: inset 0 0 0 2px rgba(255, 255, 255, 0.5); }
.a-icon { font-family: "Symbols Nerd Font"; font-size: 32px; }
.a-name { font-size: 13px; }
.key {
  font-size: 10px; color: #8a8a8f; background: rgba(0, 0, 0, 0.25);
  border-radius: 4px; padding: 0 5px;
}
button.act:hover .key, button.act.warn:focus .key, button.act.danger:focus .key { color: #ffffff; }
button.act:focus .key { color: @on_accent; }
"#;

/// "up 2 days 3 h", "up 5 h 12 min" ou "up 7 min": no máximo duas partes, como no Python.
fn uptime_text(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    let (hours, minutes) = (rem / 3600, rem % 3600 / 60);
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(ntr("%d day", "%d days", days));
    }
    if hours > 0 {
        parts.push(format!("{hours} h"));
    }
    parts.push(format!("{minutes} min"));
    parts.truncate(2);
    tr("up %s").replace("%s", &parts.join(" "))
}

fn read_uptime() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/uptime").ok()?;
    text.split_whitespace().next()?.parse::<f64>().ok().map(|s| s as u64)
}

/// O nome de quem está na sessão (o primeiro campo do GECOS, ou o login) e o login.
fn user_names() -> (String, String) {
    // SAFETY: getpwuid devolve um ponteiro para dados estáticos da libc, copiados logo em seguida, na
    // linha principal, sem outra chamada de passwd no meio
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() {
            let login = std::env::var("USER").unwrap_or_default();
            return (login.clone(), login);
        }
        let login = CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned();
        let gecos = if (*pw).pw_gecos.is_null() { String::new() } else { CStr::from_ptr((*pw).pw_gecos).to_string_lossy().into_owned() };
        (real_name(&gecos, &login), login)
    }
}

fn real_name(gecos: &str, login: &str) -> String {
    let first = gecos.split(',').next().unwrap_or("").trim();
    if first.is_empty() { login.trim().to_string() } else { first.to_string() }
}

/// A ação de uma tecla: só a letra sozinha. Sem confirmação, Super+P ou Shift+P desligariam a máquina;
/// Caps Lock e Num Lock não contam como modificadores.
fn action_for_key(name: &str, modifiers: bool) -> Option<usize> {
    if modifiers {
        return None;
    }
    ACTIONS.iter().position(|a| a.3 == name)
}

/// O próximo botão das setas, em círculo.
fn next_index(current: Option<usize>, right: bool, len: usize) -> usize {
    let i = current.unwrap_or(0);
    if right { (i + 1) % len } else { (i + len - 1) % len }
}

struct Menu {
    panel: Rc<ModalPanel>,
    buttons: Vec<gtk4::Button>,
}

impl Menu {
    fn new(main_loop: &glib::MainLoop) -> Rc<Menu> {
        let panel = ModalPanel::new(PROGRAM, "session-menu", CSS, 14, main_loop);
        let card = &panel.card;

        // Cabeçalho: usuário, máquina e há quanto tempo está ligada
        let (name, login) = user_names();
        let host = glib::host_name();
        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        head.append(&label(I_USER, &["head-icon"], 0.5));
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        col.set_valign(gtk4::Align::Center);
        col.append(&label(&name, &["title"], 0.0));
        let uptime = read_uptime().map(uptime_text).unwrap_or_default();
        col.append(&label(&format!("{login}@{host} · {uptime}"), &["sub"], 0.0));
        head.append(&col);
        card.append(&head);

        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        row.set_homogeneous(true);
        let mut buttons = Vec::new();
        for (_, text, icon, key, class) in ACTIONS {
            let button = gtk4::Button::new();
            button.add_css_class("act");
            if !class.is_empty() {
                button.add_css_class(class);
            }
            let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
            content.append(&label(icon, &["a-icon"], 0.5));
            content.append(&label(&tr(text), &["a-name"], 0.5));
            let k = label(key, &["key"], 0.5);
            k.set_halign(gtk4::Align::Center);
            content.append(&k);
            button.set_child(Some(&content));
            row.append(&button);
            buttons.push(button);
        }
        card.append(&row);
        card.append(&label(&tr("arrows and Enter · click outside or Esc to close"), &["hint"], 0.5));

        let menu = Rc::new(Menu { panel, buttons });
        for (i, button) in menu.buttons.iter().enumerate() {
            let weak = Rc::downgrade(&menu);
            button.connect_clicked(move |_| {
                if let Some(m) = weak.upgrade() {
                    m.run(i);
                }
            });
        }

        // Na captura: as setas chegam aqui antes da navegação de foco do GTK, que não anda em círculo
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(&menu);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(m) = weak.upgrade() else { return glib::Propagation::Proceed };
            m.on_key(key, state)
        });
        menu.panel.window.add_controller(keys);

        menu.panel.show();
        menu.buttons[0].grab_focus();
        menu
    }

    fn on_key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let name = key.to_lower().name().map(|n| n.to_lowercase()).unwrap_or_default();
        let modifiers = state.intersects(gtk4::accelerator_get_default_mod_mask() | gdk::ModifierType::SUPER_MASK);
        if let Some(i) = action_for_key(&name, modifiers) {
            self.run(i);
            return glib::Propagation::Stop;
        }
        if name == "left" || name == "right" {
            let focus = gtk4::prelude::GtkWindowExt::focus(&self.panel.window);
            let current = self.buttons.iter().position(|b| focus.as_ref().is_some_and(|f| f == b || f.is_ancestor(b)));
            self.buttons[next_index(current, name == "right", self.buttons.len())].grab_focus();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed // Tab, Enter e espaço: a navegação normal do GTK; Esc: o ModalPanel
    }

    /// Fecha antes (o bloqueio não deve abrir com o menu por cima) e roda a ação à parte.
    fn run(&self, i: usize) {
        if self.panel.is_closing() {
            return; // já rodou uma ação: duas ativações juntas dariam, por exemplo, dois suspends
        }
        let (action, text, ..) = ACTIONS[i];
        let log = resident::runtime_dir().join("session-action.log");
        let args = [
            "-c".to_string(),
            RUNNER.to_string(),
            bin("session-action").to_string_lossy().into_owned(),
            action.to_string(),
            tr(text),
            log.to_string_lossy().into_owned(),
            tr("Session"),
            tr("Could not run: %s."),
        ];
        if let Err(err) = spawn_detached("sh", &args) {
            eprintln!("{PROGRAM}: could not run session-action {action}: {err}");
        }
        self.panel.close(action);
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o session-menu em Python confere que o binário carrega
    }
    // Abrir de novo só fecha; não fecha os popups da barra (o menu fica por cima deles)
    swaydesk_ui::run_single(PROGRAM, &[], |_| true, |main_loop| Some(Menu::new(main_loop)), || {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_keeps_two_parts() {
        assert_eq!(uptime_text(7 * 60 + 30), "up 7 min");
        assert_eq!(uptime_text(5 * 3600 + 12 * 60), "up 5 h 12 min");
        assert_eq!(uptime_text(86400 + 3 * 3600 + 59), "up 1 day 3 h");
        assert_eq!(uptime_text(2 * 86400 + 4 * 60), "up 2 days 4 min");
        assert_eq!(uptime_text(0), "up 0 min");
    }

    #[test]
    fn the_name_comes_from_gecos_or_login() {
        assert_eq!(real_name("Nome Sobrenome,,,", "nome"), "Nome Sobrenome");
        assert_eq!(real_name(",,,", "nome"), "nome");
        assert_eq!(real_name("", "nome"), "nome");
    }

    #[test]
    fn only_the_bare_letter_runs_an_action() {
        assert_eq!(action_for_key("p", false), Some(4));
        assert_eq!(action_for_key("p", true), None);
        assert_eq!(action_for_key("l", false), Some(0));
        assert_eq!(action_for_key("x", false), None);
    }

    #[test]
    fn arrows_go_around() {
        assert_eq!(next_index(Some(4), true, 5), 0);
        assert_eq!(next_index(Some(0), false, 5), 4);
        assert_eq!(next_index(None, true, 5), 1);
    }

    #[test]
    fn same_texts_and_runner_as_python() {
        let Some(path) = swaydesk_common::reference::repo_file(env!("CARGO_MANIFEST_DIR"), "home/.local/bin/session-menu") else { return };
        let python = std::fs::read_to_string(path).unwrap();
        for (action, text, _, key, _) in ACTIONS {
            assert!(python.contains(&format!("(\"{action}\", _(\"{text}\")")), "{action}");
            assert!(python.contains(&format!("\"{key}\", ")), "{key}");
        }
        assert!(python.contains(&format!("RUNNER = \"\"\"{RUNNER}\"\"\"")));
    }
}

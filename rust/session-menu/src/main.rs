//! Menu da sessão no estilo dos popups da Waybar: o porte de `home/.local/bin/session-menu`, que passa a
//! vez a este binário (sway_rust).
//!
//! Abre no centro da tela sobre um fundo escurecido (layer-shell overlay, teclado exclusivo): Bloquear ·
//! Sair · Suspender · Reiniciar · Desligar. As teclas são as iniciais em inglês (Lock · Exit · Suspend ·
//! Reboot · Power off: l · e · s · r · p), para não depender do idioma da interface. Setas e Tab navegam,
//! Enter ativa; clicar fora ou Esc fecha. As ações passam pelo session-action, que sabe o que fazer em
//! cada sessão (Sway, Cinnamon, GNOME). Chamado por Ctrl+Alt+Delete e pelo botão de energia da Waybar.
//!
//! `--confirm reboot|poweroff`: a confirmação de um reinício ou desligamento que um app pediu ao
//! sway-sessionmgr (o "Reiniciar e atualizar" da GNOME Software), como o diálogo de fim de sessão do GNOME:
//! contagem de 60 s, "Reiniciar agora" e "Cancelar" (o foco, Esc ou clique fora). Diz se há atualização
//! offline e a trata ao confirmar (swaydesk_common::offline_updates); cancelar não mexe nela.
use std::cell::Cell;
use std::ffi::CStr;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use swaydesk_common::i18n::{ntr, tr};
use swaydesk_common::offline_updates::{self, Note, Plan, Request};
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
/// A contagem do diálogo de fim de sessão do GNOME
const CONFIRM_SECONDS: u64 = 60;
/// A saída da confirmação que não chegou a abrir (o GTK sem tela): só com ela o sway-sessionmgr age sem
/// perguntar. Qualquer outra (o GTK sai com 1 quando o Sway some, um panic dá 101) não age: depois de a
/// confirmação aparecer, sair por erro nunca pode virar um reinício que ninguém confirmou
const CONFIRM_UNAVAILABLE: u8 = 3;

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
.note { color: #dadada; font-size: 13px; }
button.choice {
  background: #38383d; border: none; box-shadow: none; color: #dadada; outline: none;
  border-radius: 10px; padding: 10px 18px; min-width: 120px;
}
button.choice:hover { background: #4c4c50; color: #ffffff; }
button.choice:focus { background: @accent; color: @on_accent; box-shadow: inset 0 0 0 2px rgba(255, 255, 255, 0.5); }
button.choice.warn:focus { background: #f27835; color: #ffffff; }
button.choice.danger:focus { background: #fc4138; color: #ffffff; }
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
        spawn_action(action, &tr(text));
        self.panel.close(action);
    }
}


/// O título, o botão, o ícone e a classe do pedido.
fn confirm_texts(request: Request) -> (String, String, &'static str, &'static str) {
    match request {
        Request::Reboot => (tr("Restart"), tr("Restart now"), "\u{f0709}", "warn"),
        Request::Shutdown => (tr("Shut down"), tr("Shut down now"), "\u{f0425}", "danger"),
    }
}

/// "O sistema vai reiniciar automaticamente em %d segundos", como no GNOME.
fn countdown_text(request: Request, seconds: u64) -> String {
    match request {
        Request::Reboot => ntr("The system will restart automatically in %d second",
                               "The system will restart automatically in %d seconds", seconds),
        Request::Shutdown => ntr("The system will power off automatically in %d second",
                                 "The system will power off automatically in %d seconds", seconds),
    }
}

fn note_text(note: Note) -> Option<String> {
    match note {
        Note::None => None,
        Note::InstallOnRestart => Some(tr("Updates will be installed during the restart.")),
        Note::InstallThenPowerOff => Some(tr("The computer will restart to install the updates and then power off.")),
        Note::InstallOnNextBoot => Some(tr("Updates will be installed the next time the computer starts.")),
    }
}

/// Os segundos que faltam, arredondados para cima: 60 logo ao abrir, 0 só no fim.
fn seconds_left(elapsed: Duration) -> u64 {
    CONFIRM_SECONDS.saturating_sub(elapsed.as_secs()).min(CONFIRM_SECONDS)
}

struct Confirm {
    panel: Rc<ModalPanel>,
    request: Request,
    plan: Plan,
    bus: Option<gtk4::gio::DBusConnection>,
    countdown: gtk4::Label,
    started: Instant,
    done: Cell<bool>,
    cancel: gtk4::Button,
    accept: gtk4::Button,
}

impl Confirm {
    fn new(main_loop: &glib::MainLoop, request: Request) -> Rc<Confirm> {
        let panel = ModalPanel::new(PROGRAM, "session-menu", CSS, 14, main_loop);
        let bus = gtk4::gio::bus_get_sync(gtk4::gio::BusType::Session, gtk4::gio::Cancellable::NONE).ok();
        let plan = offline_updates::plan(request, offline_updates::query(bus.as_ref()));
        let (title, button, icon, class) = confirm_texts(request);
        let card = &panel.card;

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        head.append(&label(icon, &["head-icon"], 0.5));
        let col = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        col.set_valign(gtk4::Align::Center);
        col.append(&label(&title, &["title"], 0.0));
        let countdown = label(&countdown_text(request, CONFIRM_SECONDS), &["sub"], 0.0);
        col.append(&countdown);
        head.append(&col);
        card.append(&head);
        if let Some(text) = note_text(plan.note) {
            let note = label(&text, &["note"], 0.0);
            note.set_wrap(true);
            note.set_max_width_chars(48);
            card.append(&note);
        }

        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        row.set_homogeneous(true);
        row.set_halign(gtk4::Align::End);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.add_css_class("choice");
        let accept = gtk4::Button::with_label(&button);
        accept.add_css_class("choice");
        accept.add_css_class(class);
        row.append(&cancel);
        row.append(&accept);
        card.append(&row);
        let hint = if plan.kept_on_cancel {
            tr("If you cancel, the updates stay downloaded; nothing is installed now.")
        } else {
            tr("click outside or Esc to cancel")
        };
        let hint = label(&hint, &["hint"], 0.5);
        hint.set_wrap(true);
        hint.set_max_width_chars(48);
        card.append(&hint);

        let confirm = Rc::new(Confirm { panel, request, plan, bus, countdown, started: Instant::now(), done: Cell::new(false), cancel, accept });
        let weak = Rc::downgrade(&confirm);
        confirm.cancel.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.panel.close("cancelar");
            }
        });
        let weak = Rc::downgrade(&confirm);
        confirm.accept.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.run("agora");
            }
        });

        // As setas trocam de botão; sem as letras do menu: aqui uma tecla solta não age
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(&confirm);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(c) = weak.upgrade() else { return glib::Propagation::Proceed };
            if matches!(key, gdk::Key::Left | gdk::Key::Right | gdk::Key::KP_Left | gdk::Key::KP_Right) {
                let target = if c.cancel.has_focus() { &c.accept } else { &c.cancel };
                target.grab_focus();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        confirm.panel.window.add_controller(keys);

        let weak = Rc::downgrade(&confirm);
        glib::timeout_add_local(Duration::from_millis(250), move || {
            let Some(c) = weak.upgrade() else { return glib::ControlFlow::Break };
            if c.panel.is_closing() {
                return glib::ControlFlow::Break;
            }
            let left = seconds_left(c.started.elapsed());
            if left == 0 {
                c.run("contagem");
                return glib::ControlFlow::Break;
            }
            c.countdown.set_text(&countdown_text(c.request, left));
            glib::ControlFlow::Continue
        });

        confirm.panel.show();
        // O foco no Cancelar: um Enter digitado para outra janela não reinicia a máquina
        confirm.cancel.grab_focus();
        confirm
    }

    /// Esconde o diálogo, trata a atualização offline (o SetAction) e roda a ação à parte, como o menu.
    fn run(&self, reason: &str) {
        if self.panel.is_closing() || self.done.replace(true) {
            return;
        }
        self.panel.window.set_visible(false);
        let action = offline_updates::confirm(self.bus.as_ref(), &self.plan);
        let (title, ..) = confirm_texts(self.request);
        spawn_action(action, &title);
        self.panel.close(&format!("{reason}: {action}"));
    }
}

/// O session-action à parte, pelo RUNNER: avisa se falhar.
fn spawn_action(action: &str, text: &str) {
    let log = resident::runtime_dir().join("session-action.log");
    let args = [
        "-c".to_string(),
        RUNNER.to_string(),
        bin("session-action").to_string_lossy().into_owned(),
        action.to_string(),
        text.to_string(),
        log.to_string_lossy().into_owned(),
        tr("Session"),
        tr("Could not run: %s."),
    ];
    if let Err(err) = spawn_detached("sh", &args) {
        eprintln!("{PROGRAM}: could not run session-action {action}: {err}");
    }
}

/// O pedido do `--confirm`: Some(Ok) com a ação, Some(Err) se ela não existe, None sem o `--confirm`.
fn confirm_arg(args: &[String]) -> Option<Result<Request, String>> {
    let at = args.iter().position(|a| a == "--confirm")?;
    let action = args.get(at + 1).cloned().unwrap_or_default();
    Some(Request::from_action(&action).ok_or(action))
}

const USAGE: &str = "usage: swaydesk-session-menu [--confirm reboot|poweroff]\n\
    Opens the session menu (lock, log out, suspend, reboot, power off); with --confirm, only the confirmation.";

fn main() -> ExitCode {
    // O --help/-h só mostra o uso: sem isso, ele abria o menu (visto ao rodar à mão, num Sway comum)
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o session-menu em Python confere que o binário carrega
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let confirm = match confirm_arg(&args) {
        Some(Err(action)) => {
            eprintln!("{PROGRAM}: --confirm takes reboot or poweroff, not '{action}'");
            return ExitCode::from(2);
        }
        Some(Ok(request)) => Some(request),
        None => None,
    };
    // Abrir o menu de novo só fecha; não fecha os popups da barra (o menu fica por cima deles). A
    // confirmação sempre abre: no lugar do menu, ou de outra confirmação (que é cancelada, sem agir), e o
    // menu aberto sobre uma confirmação a cancela. Sem a trava em 2 s (outro pedido abrindo junto), sai
    // com 0 sem mostrar nada: o pedido se perde, mas sem agir, e a atualização agendada continua
    let shown = Rc::new(Cell::new(false));
    let built = shown.clone();
    let code = swaydesk_ui::run_single(PROGRAM, &[], |open| confirm.is_none() && confirm_arg(open).is_none(), |main_loop| {
        let window: Rc<dyn std::any::Any> = match confirm {
            Some(request) => Confirm::new(main_loop, request),
            None => Menu::new(main_loop),
        };
        built.set(true);
        Some(window)
    }, || {});
    if confirm.is_some() && !shown.get() && code == ExitCode::FAILURE {
        return ExitCode::from(CONFIRM_UNAVAILABLE); // o GTK não iniciou: nada apareceu
    }
    code
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
    fn the_confirm_argument() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(confirm_arg(&args(&["--confirm", "reboot"])), Some(Ok(Request::Reboot)));
        assert_eq!(confirm_arg(&args(&["--confirm", "poweroff"])), Some(Ok(Request::Shutdown)));
        assert_eq!(confirm_arg(&args(&["--confirm", "suspend"])), Some(Err("suspend".into())));
        assert_eq!(confirm_arg(&args(&["--confirm"])), Some(Err(String::new())));
        assert_eq!(confirm_arg(&args(&[])), None);
    }

    #[test]
    fn the_countdown_starts_at_60_and_ends_at_0() {
        assert_eq!(seconds_left(Duration::ZERO), 60);
        assert_eq!(seconds_left(Duration::from_millis(999)), 60);
        assert_eq!(seconds_left(Duration::from_millis(59_001)), 1);
        assert_eq!(seconds_left(Duration::from_secs(60)), 0);
        assert_eq!(seconds_left(Duration::from_secs(90)), 0);
        assert_eq!(countdown_text(Request::Reboot, 1), "The system will restart automatically in 1 second");
        assert_eq!(countdown_text(Request::Shutdown, 42), "The system will power off automatically in 42 seconds");
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
        for text in ["Restart now", "Shut down now", "Cancel", "Updates will be installed during the restart.",
                     "The computer will restart to install the updates and then power off.",
                     "Updates will be installed the next time the computer starts.",
                     "If you cancel, the updates stay downloaded; nothing is installed now.",
                     "click outside or Esc to cancel", "The system will restart automatically in %d seconds",
                     "The system will power off automatically in %d seconds"] {
            assert!(python.contains(&format!("\"{text}\"")), "{text}");
        }
        assert!(python.contains(&format!("CONFIRM_SECONDS = {CONFIRM_SECONDS}")));
        assert!(python.contains(&format!("CONFIRM_UNAVAILABLE = {CONFIRM_UNAVAILABLE}")));
    }
}

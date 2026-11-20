//! A atualização offline pendente (a do "Reiniciar e atualizar" da GNOME Software), para o reinício e o
//! desligamento pedidos pelo sway-sessionmgr: o porte de `home/.local/bin/sway_offline_updates.py`.
//!
//! Como o diálogo de fim de sessão do gnome-shell: o estado vem da interface org.gnome.Software.OfflineUpdates
//! (GNOME Software 50 em diante) e, sem ela, dos arquivos do PackageKit. Com a interface, um desligamento com
//! atualização vira reinício com SetAction("shutdown"): o computador instala e desliga em seguida. Sem ela, ou
//! se o SetAction falhar, desliga como pedido, e a atualização entra no próximo boot. Cancelar não mexe em
//! nada: a atualização agendada continua para o próximo reinício.
use std::path::Path;
use std::time::Duration;

const SOFTWARE: &str = "org.gnome.Software";
const SOFTWARE_PATH: &str = "/org/gnome/Software/OfflineUpdates";
const SOFTWARE_IFACE: &str = "org.gnome.Software.OfflineUpdates";
/// O link que o PackageKit cria ao agendar (pk_offline_trigger): o systemd entra no system-update.target
const SYSTEM_UPDATE: &str = "/system-update";
/// A atualização baixada e pronta, ainda sem agendar
const PREPARED: &str = "/var/lib/PackageKit/prepared-update";
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    None,
    /// Pronta, mas só entra se alguém agendar (o SetAction)
    Prepared,
    /// Entra no próximo reinício
    Scheduled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Updates {
    pub state: State,
    /// Se o estado veio da GNOME Software, que aceita o SetAction
    pub via_software: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Reboot,
    Shutdown,
}

impl Request {
    pub fn from_action(action: &str) -> Option<Request> {
        match action {
            "reboot" => Some(Request::Reboot),
            "poweroff" => Some(Request::Shutdown),
            _ => None,
        }
    }
}

/// O que o diálogo diz sobre a atualização.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    None,
    /// "As atualizações serão instaladas durante o reinício."
    InstallOnRestart,
    /// "O computador vai reiniciar para instalar as atualizações e depois desligar."
    InstallThenPowerOff,
    /// "As atualizações serão instaladas na próxima vez que o computador ligar."
    InstallOnNextBoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// A ação do session-action se o SetAction não for chamado ou falhar
    pub action: &'static str,
    /// O SetAction da GNOME Software ao confirmar
    pub set_action: Option<&'static str>,
    /// Com o SetAction aceito, a ação passa a ser esta
    pub action_if_set: &'static str,
    pub note: Note,
    /// Se a atualização continua agendada depois de cancelar: o diálogo diz "Se cancelar, …continuam baixadas…"
    pub kept_on_cancel: bool,
}

/// O plano de um pedido, como o _confirm do endSessionDialog.js com a caixa "Instalar atualizações" marcada.
pub fn plan(request: Request, updates: Updates) -> Plan {
    let pending = updates.state != State::None;
    let kept_on_cancel = updates.state == State::Scheduled;
    match request {
        Request::Reboot => Plan {
            action: "reboot",
            set_action: (updates.via_software && pending).then_some("reboot"),
            action_if_set: "reboot",
            note: if updates.state == State::Scheduled || (updates.via_software && pending) { Note::InstallOnRestart } else { Note::None },
            kept_on_cancel,
        },
        Request::Shutdown if updates.via_software && pending => Plan {
            action: "poweroff",
            set_action: Some("shutdown"),
            action_if_set: "reboot",
            note: Note::InstallThenPowerOff,
            kept_on_cancel,
        },
        Request::Shutdown => Plan {
            action: "poweroff",
            set_action: None,
            action_if_set: "poweroff",
            note: if updates.state == State::Scheduled { Note::InstallOnNextBoot } else { Note::None },
            kept_on_cancel,
        },
    }
}

/// O estado pelos arquivos do PackageKit: `root` é a raiz (/ fora dos testes).
pub fn state_from_files(root: &Path) -> State {
    let path = |p: &str| root.join(p.trim_start_matches('/'));
    // symlink_metadata: o /system-update é um link para a pasta do PackageKit, que pode não ser legível
    if std::fs::symlink_metadata(path(SYSTEM_UPDATE)).is_ok() {
        State::Scheduled
    } else if path(PREPARED).exists() {
        State::Prepared
    } else {
        State::None
    }
}

pub fn state_from_software(answer: &str) -> Option<State> {
    match answer {
        "none" => Some(State::None),
        "prepared" => Some(State::Prepared),
        "scheduled" => Some(State::Scheduled),
        _ => None,
    }
}

fn call(bus: &gio::DBusConnection, method: &str, args: Option<&gio::glib::Variant>) -> Result<gio::glib::Variant, gio::glib::Error> {
    bus.call_sync(Some(SOFTWARE), SOFTWARE_PATH, SOFTWARE_IFACE, method, args, None,
                  gio::DBusCallFlags::NO_AUTO_START, TIMEOUT.as_millis() as i32, gio::Cancellable::NONE)
}

/// O estado agora: o da GNOME Software, se ela estiver no barramento e tiver a interface; senão, o dos arquivos.
pub fn query(bus: Option<&gio::DBusConnection>) -> Updates {
    let software = bus.and_then(|bus| call(bus, "GetState", None).ok())
        .and_then(|v| v.get::<(String,)>())
        .and_then(|(s,)| state_from_software(&s));
    match software {
        Some(state) => Updates { state, via_software: true },
        None => Updates { state: state_from_files(Path::new("/")), via_software: false },
    }
}

/// O que fazer ao confirmar: chama o SetAction do plano e devolve a ação do session-action.
pub fn confirm(bus: Option<&gio::DBusConnection>, plan: &Plan) -> &'static str {
    let Some(action) = plan.set_action else { return plan.action };
    let Some(bus) = bus else { return plan.action };
    match call(bus, "SetAction", Some(&(action,).into())) {
        Ok(_) => plan.action_if_set,
        Err(err) => {
            eprintln!("swaydesk: GNOME Software SetAction({action}): {err}");
            plan.action
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn up(state: State, via_software: bool) -> Updates {
        Updates { state, via_software }
    }

    #[test]
    fn reboot_installs_what_is_scheduled() {
        let p = plan(Request::Reboot, up(State::Scheduled, false));
        assert_eq!((p.action, p.set_action, p.note, p.kept_on_cancel), ("reboot", None, Note::InstallOnRestart, true));
        let p = plan(Request::Reboot, up(State::Scheduled, true));
        assert_eq!((p.set_action, p.note), (Some("reboot"), Note::InstallOnRestart));
        // Pronta e sem agendar: só a GNOME Software agenda (o SetAction); pelos arquivos, não entra
        assert_eq!(plan(Request::Reboot, up(State::Prepared, true)).note, Note::InstallOnRestart);
        assert!(!plan(Request::Reboot, up(State::Prepared, true)).kept_on_cancel);
        assert_eq!(plan(Request::Reboot, up(State::Prepared, false)).note, Note::None);
        assert_eq!(plan(Request::Reboot, up(State::None, true)), Plan {
            action: "reboot", set_action: None, action_if_set: "reboot", note: Note::None, kept_on_cancel: false });
    }

    #[test]
    fn shutdown_with_updates_restarts_to_install_them() {
        let p = plan(Request::Shutdown, up(State::Scheduled, true));
        assert_eq!((p.action, p.set_action, p.action_if_set, p.note), ("poweroff", Some("shutdown"), "reboot", Note::InstallThenPowerOff));
        // Sem a interface (GNOME Software 48, no Debian 13): desliga, e a atualização entra no próximo boot
        let p = plan(Request::Shutdown, up(State::Scheduled, false));
        assert_eq!((p.action, p.set_action, p.note), ("poweroff", None, Note::InstallOnNextBoot));
        assert_eq!(plan(Request::Shutdown, up(State::None, true)).set_action, None);
        assert_eq!(plan(Request::Shutdown, up(State::None, false)).note, Note::None);
    }

    #[test]
    fn the_files_say_scheduled_or_prepared() {
        let dir = std::env::temp_dir().join(format!("offline-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("var/lib/PackageKit")).unwrap();
        assert_eq!(state_from_files(&dir), State::None);
        std::fs::write(dir.join("var/lib/PackageKit/prepared-update"), "").unwrap();
        assert_eq!(state_from_files(&dir), State::Prepared);
        // Um link quebrado conta: a pasta de destino pode não ser legível
        std::os::unix::fs::symlink(dir.join("nao-existe"), dir.join("system-update")).unwrap();
        assert_eq!(state_from_files(&dir), State::Scheduled);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn software_states() {
        assert_eq!(state_from_software("scheduled"), Some(State::Scheduled));
        assert_eq!(state_from_software("prepared"), Some(State::Prepared));
        assert_eq!(state_from_software("none"), Some(State::None));
        assert_eq!(state_from_software("outro"), None);
        assert_eq!(Request::from_action("poweroff"), Some(Request::Shutdown));
        assert_eq!(Request::from_action("suspend"), None);
    }
}

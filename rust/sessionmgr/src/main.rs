//! O mínimo do org.gnome.SessionManager no Sway: o "Reiniciar e atualizar" da GNOME Software. O porte do
//! `sway-sessionmgr` em Python.
//!
//! Só o Reboot e o Shutdown respondem; o resto dá UnknownMethod, para nenhum app achar que está no GNOME.
//! Roda só na sessão Sway (exec no config, nunca pela ativação do D-Bus) e sai junto com ela: o barramento
//! da sessão sobrevive ao logout, e um GNOME aberto depois precisa do nome livre.
//! --check: só confere que o binário carrega.
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use gio::glib;
use swaydesk_common::resident;

const PROGRAM: &str = "sway-sessionmgr";
const NAME: &str = "org.gnome.SessionManager";
const PATH: &str = "/org/gnome/SessionManager";
const XML: &str = r#"<node>
  <interface name="org.gnome.SessionManager">
    <method name="Reboot"/>
    <method name="Shutdown"/>
  </interface>
</node>"#;
/// A resposta sai antes: a GNOME Software cancela a atualização se a chamada falhar
const DELAY: Duration = Duration::from_millis(300);

/// O que cada método faz, pelo session-action, que no Sway chama o systemctl
fn action(method: &str) -> Option<&'static str> {
    match method {
        "Reboot" => Some("reboot"),
        "Shutdown" => Some("poweroff"),
        _ => None,
    }
}

/// O session-action do pacote (/usr/libexec/swaydesk) ou o da pasta do binário (o ~/.local/bin dos
/// dotfiles), como o Python usa o da pasta do script. Só com os pacotes, a pasta do binário é o /usr/bin, sem
/// o script: o reiniciar e o desligar falhavam. Nunca o do PATH: num teste, o de verdade desligaria a
/// máquina.
fn session_action() -> Option<PathBuf> {
    swaydesk_common::paths::helper_beside_exe("session-action")
}

fn act(action: &str) {
    let Some(script) = session_action() else {
        eprintln!("{PROGRAM}: could not find session-action");
        return;
    };
    if let Err(err) = Command::new(&script).arg(action).status() {
        eprintln!("{PROGRAM}: could not run {}: {err}", script.display());
    }
}

fn on_call(method: &str, invocation: gio::DBusMethodInvocation) {
    let Some(action) = action(method) else {
        invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", &format!("{method} does not exist in Sway"));
        return;
    };
    let sender = invocation.sender().map(|s| s.to_string()).unwrap_or_default();
    eprintln!("{PROGRAM}: {method} requested by {sender}");
    invocation.return_value(None);
    glib::timeout_add_local_once(DELAY, move || act(action));
}

/// Sai quando o Sway sai: o evento de saída, ou a conexão que cai se ele morrer.
fn watch_sway(main_loop: glib::MainLoop) {
    std::thread::spawn(move || {
        let _ = Command::new("swaymsg").args(["-t", "subscribe", r#"["shutdown"]"#])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        main_loop.quit(); // o g_main_loop_quit vale de outra linha
    });
}

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    let main_loop = glib::MainLoop::new(None, false);
    let bus = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!("{PROGRAM}: no session bus: {err}");
            return ExitCode::FAILURE;
        }
    };
    let node = gio::DBusNodeInfo::for_xml(XML).expect("interface XML");
    let iface = node.lookup_interface("org.gnome.SessionManager").expect("interface");
    let registered = bus
        .register_object(PATH, &iface)
        .method_call(|_conn, _sender, _path, _iface, method, _params, invocation| on_call(method, invocation))
        .build();
    if let Err(err) = registered {
        eprintln!("{PROGRAM}: register object: {err}");
        return ExitCode::FAILURE;
    }
    // Sem substituir quem já tem o nome (um gnome-session de verdade) e saindo se o perder
    let lost = main_loop.clone();
    let _owner = gio::bus_own_name_on_connection(&bus, NAME, gio::BusNameOwnerFlags::DO_NOT_QUEUE, |_, _| {},
                                                  move |_, _| lost.quit());
    watch_sway(main_loop.clone());
    main_loop.run();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reboot_and_shutdown_act() {
        assert_eq!(action("Reboot"), Some("reboot"));
        assert_eq!(action("Shutdown"), Some("poweroff"));
        assert_eq!(action("Logout"), None);
    }
}

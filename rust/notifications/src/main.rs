//! Ícone das notificações na Waybar (custom/notification), lido do sway-notifd: o porte de
//! `home/.local/bin/waybar-notifications`, que passa a vez a este binário (sway_rust).
//!
//! Imprime uma linha JSON a cada mudança; os nomes em "alt" são os de format-icons na config. Volta
//! sozinho quando o daemon reinicia.
//!
//! uso: swaydesk-notifications [toggle-dnd | --check]
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gio::glib;
use gio::prelude::*;
use serde_json::{json, Value};
use swaydesk_common::text::strip_remote_error;
use swaydesk_notifd_logic::icon_state;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const CONTROL: &str = "local.SwayDesktop.Notifications1";
const CALL_MS: i32 = 3000;
const PROGRAM: &str = "waybar-notifications";

fn offline() -> Value {
    json!({"items": [], "dnd": {"active": false}})
}

/// A linha para a Waybar.
fn line(snap: &Value, available: bool) -> Value {
    let alt = icon_state(snap);
    let count = snap["items"].as_array().map_or(0, Vec::len);
    json!({"text": count.to_string(), "alt": alt, "tooltip": "", "class": if available { alt } else { "offline" }})
}

fn emit(snap: &Value, available: bool) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", line(snap, available)).and_then(|()| out.flush()).is_err() {
        std::process::exit(0); // a Waybar foi embora
    }
}

fn session_bus() -> Result<gio::DBusConnection, glib::Error> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
}


fn toggle_dnd() -> Result<(), glib::Error> {
    // Sem iniciar nada: o arquivo de ativação do pacote aponta para o swaync
    session_bus()?.call_sync(
        Some(NAME),
        PATH,
        CONTROL,
        "SetDnd",
        Some(&("toggle",).to_variant()),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        CALL_MS,
        gio::Cancellable::NONE,
    )?;
    Ok(())
}

struct Icon {
    bus: gio::DBusConnection,
    pending: Cell<bool>,
    /// (estado, quantas, disponível): só imprime quando muda
    last: RefCell<Option<(&'static str, usize, bool)>>,
}

impl Icon {
    fn update(self: &Rc<Self>) {
        self.pending.set(false);
        let icon = self.clone();
        self.bus.call(
            Some(NAME),
            PATH,
            CONTROL,
            "List",
            None,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            CALL_MS,
            gio::Cancellable::NONE,
            move |result| {
                let parsed = result
                    .ok()
                    .and_then(|reply| reply.child_value(0).str().map(str::to_string))
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok());
                let (snap, available) = match parsed {
                    Some(snap) => (snap, true),
                    None => (offline(), false),
                };
                let count = snap["items"].as_array().map_or(0, Vec::len);
                let key = (icon_state(&snap), count, available);
                if icon.last.borrow().as_ref() != Some(&key) {
                    *icon.last.borrow_mut() = Some(key);
                    emit(&snap, available);
                }
            },
        );
    }

    /// Uma rajada de mudanças vira uma leitura só.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let icon = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(100), move || icon.update());
        }
    }
}

fn run() -> Result<(), glib::Error> {
    let bus = session_bus()?;
    let icon = Rc::new(Icon { bus: bus.clone(), pending: Cell::new(false), last: RefCell::new(None) });
    let i = icon.clone();
    #[allow(deprecated)] // o signal_subscribe com closure continua o caminho simples no gio 0.20
    bus.signal_subscribe(None, Some(CONTROL), Some("Changed"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| {
        i.schedule()
    });
    let i = icon.clone();
    #[allow(deprecated)]
    bus.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(NAME),
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, _| i.schedule(),
    );
    // Sem escrever nada, o POLLERR é o único aviso de que a Waybar foi embora
    glib::timeout_add_seconds_local(10, || {
        let mut fd = libc::pollfd { fd: 1, events: 0, revents: 0 };
        if unsafe { libc::poll(&mut fd, 1, 0) } > 0 {
            std::process::exit(0);
        }
        glib::ControlFlow::Continue
    });
    icon.update();
    glib::MainLoop::new(None, false).run();
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--check"] => return ExitCode::SUCCESS, // o waybar-notifications em Python confere que o binário carrega
        ["toggle-dnd"] => toggle_dnd(),
        [] => run(),
        _ => {
            eprintln!("uso: {PROGRAM} [toggle-dnd]");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{PROGRAM}: {}", strip_remote_error(err.message()));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_follows_the_state() {
        let snap = json!({"items": [{"id": 1}], "dnd": {"active": true}});
        assert_eq!(line(&snap, true), json!({"text": "1", "alt": "dnd-notification", "tooltip": "", "class": "dnd-notification"}));
        assert_eq!(line(&offline(), false), json!({"text": "0", "alt": "none", "tooltip": "", "class": "offline"}));
    }
}

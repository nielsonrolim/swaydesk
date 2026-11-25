//! Ícone de Bluetooth da Waybar (custom/bluetooth): o porte de `home/.local/bin/waybar-bluetooth`, que
//! passa a vez a este binário (sway_rust). Uma linha JSON a cada mudança, lida do BlueZ pelo D-Bus; relê
//! também a cada 10 s. O popup manda SIGUSR1 ao abrir e ao fechar: com ele aberto a dica fica vazia.
//!
//! O nome do aparelho vem de fora, e o texto e a dica da Waybar são marcação Pango: passam por escape.
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gio::glib;
use swaydesk_common::{config, i18n, resident};
use swaydesk_bluez as bluez;

const PROGRAM: &str = "waybar-bluetooth";

/// A linha da Waybar: ícone, dica e classe, com escape.
fn line(state: Option<&bluez::State>, popup_open: bool) -> String {
    let (icon, class, lines) = bluez::icon_state(state);
    let tooltip = if popup_open {
        String::new()
    } else {
        lines.iter().filter(|l| !l.is_empty()).map(|l| glib::markup_escape_text(l).to_string()).collect::<Vec<_>>().join("\n")
    };
    serde_json::json!({"text": glib::markup_escape_text(icon).as_str(), "tooltip": tooltip, "class": class}).to_string()
}

fn popup_open() -> bool {
    resident::live_pid(&resident::pidfile("waybar-bluetooth-popup"), "waybar-bluetooth-popup").is_some()
}

struct Icon {
    bus: gio::DBusConnection,
    last: RefCell<Option<String>>,
    pending: Cell<bool>,
    reading: Cell<bool>,
    again: Cell<bool>,
}

impl Icon {
    /// Lê numa linha bloqueante do GIO e escreve se mudou (ou sempre, com `force`).
    fn update(self: &Rc<Self>, force: bool) {
        if self.reading.replace(true) {
            self.again.set(true);
            return;
        }
        let (icon, bus) = (self.clone(), self.bus.clone());
        glib::MainContext::default().spawn_local(async move {
            let state = gio::spawn_blocking(move || bluez::read(&bus)).await.ok().flatten();
            icon.reading.set(false);
            let current = line(state.as_ref(), popup_open());
            if force || icon.last.borrow().as_deref() != Some(current.as_str()) {
                let mut out = std::io::stdout();
                if writeln!(out, "{current}").and_then(|_| out.flush()).is_err() {
                    std::process::exit(0); // a Waybar foi embora
                }
                icon.last.replace(Some(current));
            }
            if icon.again.replace(false) {
                icon.schedule();
            }
        });
    }

    /// Uma rajada de avisos (durante uma busca o BlueZ manda muitos) vira uma leitura só.
    fn schedule(self: &Rc<Self>) {
        if !self.pending.replace(true) {
            let icon = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(200), move || {
                icon.pending.set(false);
                icon.update(false);
            });
        }
    }
}

fn run() -> ExitCode {
    // Ignorado antes do arquivo do PID: o popup pode avisar logo, e a ação padrão encerraria o processo.
    // O tratador de verdade (unix_signal_add) vem abaixo
    // SAFETY: troca a ação de um sinal antes de qualquer linha
    unsafe { libc::signal(libc::SIGUSR1, libc::SIG_IGN) };
    i18n::init(&config::load());
    let pidfile = resident::pidfile(PROGRAM);
    let _ = std::fs::write(&pidfile, std::process::id().to_string());
    let bus = match bluez::system_bus() {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!("{PROGRAM}: {}", err.message());
            return ExitCode::FAILURE;
        }
    };
    let icon = Rc::new(Icon { bus: bus.clone(), last: RefCell::new(None), pending: Cell::new(false), reading: Cell::new(false), again: Cell::new(false) });
    let i = icon.clone();
    let _subscriptions = bluez::watch(&bus, move || i.schedule());
    let i = icon.clone();
    glib::unix_signal_add_local(libc::SIGUSR1, move || {
        i.update(true);
        glib::ControlFlow::Continue
    });
    // A Waybar reiniciada fecha a ponta de leitura do pipe; sem escrever nada, o POLLERR é o único aviso
    let i = icon.clone();
    glib::timeout_add_seconds_local(10, move || {
        let mut fd = libc::pollfd { fd: 1, events: 0, revents: 0 };
        // SAFETY: poll num fd, sem ponteiros guardados
        if unsafe { libc::poll(&mut fd, 1, 0) } > 0 {
            std::process::exit(0);
        }
        i.update(false);
        glib::ControlFlow::Continue
    });
    icon.update(false);
    glib::MainLoop::new(None, false).run();
    ExitCode::SUCCESS
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-bluetooth\n",
    "    The Bluetooth icon for Waybar (custom/bluetooth): one JSON line per change, from BlueZ.",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().nth(1).as_deref() == Some("--check") {
        return ExitCode::SUCCESS; // o waybar-bluetooth em Python confere que o binário carrega
    }
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_escaped_and_the_tooltip_hides_with_the_popup() {
        let state = bluez::State {
            adapter: Some(bluez::Adapter { path: "/h".into(), powered: true, ..Default::default() }),
            devices: vec![bluez::Device { alias: "Fone <&>".into(), connected: true, battery: Some(40), ..Default::default() }],
        };
        let v: serde_json::Value = serde_json::from_str(&line(Some(&state), false)).unwrap();
        assert_eq!((v["class"].as_str(), v["tooltip"].as_str()), (Some("connected"), Some("Fone &lt;&amp;&gt; · 40%")));
        let v: serde_json::Value = serde_json::from_str(&line(Some(&state), true)).unwrap();
        assert_eq!(v["tooltip"], "");
        let v: serde_json::Value = serde_json::from_str(&line(None, false)).unwrap();
        assert_eq!(v["class"], "off");
    }
}

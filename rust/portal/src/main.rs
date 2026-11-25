//! A cor de destaque da config para os apps libadwaita (Nautilus, Configurações), pelo portal. O porte do
//! `sway-portal` em Python.
//!
//! Este backend responde só org.freedesktop.appearance accent-color; o xdg-desktop-portal junta a resposta
//! com a do gtk (sway-portals.conf: Settings=swaydesktop;gtk), que dá o resto. Sobe pela ativação do D-Bus
//! e avisa os apps quando o config.toml muda. Sai junto com o Sway: ativado pelo D-Bus, ele é filho do
//! systemd do usuário e ficava vivo depois do logout. --check: só confere que o binário carrega.
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gio::glib;
use gio::prelude::*;
use swaydesk_common::{config, resident};

const PROGRAM: &str = "sway-portal";
const NAME: &str = "org.freedesktop.impl.portal.desktop.swaydesktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.impl.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "accent-color";
const XML: &str = r#"<node>
  <interface name="org.freedesktop.impl.portal.Settings">
    <method name="ReadAll">
      <arg type="as" name="namespaces" direction="in"/>
      <arg type="a{sa{sv}}" name="value" direction="out"/>
    </method>
    <method name="Read">
      <arg type="s" name="namespace" direction="in"/>
      <arg type="s" name="key" direction="in"/>
      <arg type="v" name="value" direction="out"/>
    </method>
    <signal name="SettingChanged">
      <arg type="s" name="namespace"/>
      <arg type="s" name="key"/>
      <arg type="v" name="value"/>
    </signal>
    <property name="version" type="u" access="read"/>
  </interface>
</node>"#;

type Rgb = (f64, f64, f64);

/// #rrggbb em três frações de 0 a 1, o formato do portal. A config já validou a cor.
fn rgb(color: &str) -> Rgb {
    let part = |i: usize| f64::from(color.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(0)) / 255.0;
    (part(1), part(3), part(5))
}

/// Os padrões do ReadAll: lista vazia é tudo, e um "*" no fim casa pelo começo do nome.
fn asks_for_appearance(namespaces: &[String]) -> bool {
    namespaces.is_empty()
        || namespaces.iter().any(|p| p == NAMESPACE || p.strip_suffix('*').is_some_and(|prefix| NAMESPACE.starts_with(prefix)))
}

fn accent() -> Rgb {
    rgb(&config::load_config(&config::path()).0.accent)
}

fn on_call(method: &str, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
    match method {
        "ReadAll" => {
            let namespaces = params.get::<(Vec<String>,)>().map(|p| p.0).unwrap_or_default();
            let mut found: HashMap<String, HashMap<String, glib::Variant>> = HashMap::new();
            if asks_for_appearance(&namespaces) {
                found.insert(NAMESPACE.to_owned(), HashMap::from([(KEY.to_owned(), accent().to_variant())]));
            }
            invocation.return_value(Some(&(found,).to_variant()));
        }
        "Read" if params.get::<(String, String)>().is_some_and(|(ns, key)| ns == NAMESPACE && key == KEY) => {
            invocation.return_value(Some(&(accent().to_variant(),).to_variant()));
        }
        // O portal pergunta a cada backend pelo que ele não tem: o gtk responde o resto
        _ => invocation.return_dbus_error("org.freedesktop.portal.Error.NotFound", "Requested setting not found"),
    }
}

/// Uma assinatura que durou menos que isso não chegou a conectar (sem Sway, ou o SWAYSOCK de uma sessão
/// que já acabou): aí o portal fica, como antes, para não recusar o pedido que o ativou.
const CONNECTED: Duration = Duration::from_secs(1);

/// Sai quando o Sway sai: o evento de saída, ou a conexão que cai se ele morrer.
fn watch_sway(main_loop: glib::MainLoop) {
    std::thread::spawn(move || {
        let start = Instant::now();
        let _ = Command::new("swaymsg").args(["-t", "subscribe", r#"["shutdown"]"#])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        if start.elapsed() >= CONNECTED {
            main_loop.quit(); // o g_main_loop_quit vale de outra linha
        }
    });
}

/// O --help (swaydesk_common::early_args!)
const USAGE: &str = concat!(
    "usage: swaydesk-portal\n",
    "    The Settings portal backend with the accent color of the desktop config (started by D-Bus).",
);

fn main() -> ExitCode {
    swaydesk_common::early_args!(USAGE);
    if std::env::args().skip(1).any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    let main_loop = glib::MainLoop::new(None, false);
    let node = gio::DBusNodeInfo::for_xml(XML).expect("interface XML");
    let iface = node.lookup_interface(INTERFACE).expect("interface");
    let conn: Rc<RefCell<Option<gio::DBusConnection>>> = Rc::default();
    let last = Rc::new(RefCell::new(accent()));

    // Pelo nome, não pelo arquivo: gravar a config troca o arquivo inteiro
    let monitor = gio::File::for_path(config::path()).monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE);
    let _monitor = match monitor {
        Ok(monitor) => {
            let (conn, last) = (conn.clone(), last.clone());
            monitor.connect_changed(move |_, _, _, _| {
                let value = accent();
                let Some(c) = conn.borrow().clone() else { return };
                if *last.borrow() == value {
                    return;
                }
                last.replace(value);
                let args = (NAMESPACE, KEY, value.to_variant()).to_variant();
                if let Err(err) = c.emit_signal(None, PATH, INTERFACE, "SettingChanged", Some(&args)) {
                    eprintln!("{PROGRAM}: SettingChanged: {err}");
                }
            });
            Some(monitor)
        }
        Err(err) => {
            eprintln!("{PROGRAM}: cannot watch the config: {err}");
            None
        }
    };
    let on_bus = conn.clone();
    let lost = main_loop.clone();
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |c, _| {
            let registered = c
                .register_object(PATH, &iface)
                .method_call(|_conn, _sender, _path, _iface, method, params, invocation| on_call(method, &params, invocation))
                .property(|_conn, _sender, _path, _iface, prop| {
                    if prop == "version" { 1u32.to_variant() } else { ().to_variant() }
                })
                .build();
            match registered {
                Ok(_) => *on_bus.borrow_mut() = Some(c),
                Err(err) => eprintln!("{PROGRAM}: register object: {err}"),
            }
        },
        |_, _| {},
        move |_, _| lost.quit(),
    );
    watch_sway(main_loop.clone());
    main_loop.run();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_as_fractions() {
        assert_eq!(rgb("#ff0080"), (1.0, 0.0, 128.0 / 255.0));
    }

    #[test]
    fn namespaces_that_include_the_appearance() {
        let cases: &[(&[&str], bool)] = &[(&[], true), (&["org.freedesktop.appearance"], true), (&["org.freedesktop.*"], true),
            (&["*"], true), (&["org.gnome.*"], false), (&["org.freedesktop.appearance.x"], false)];
        for (patterns, expected) in cases {
            let patterns: Vec<String> = patterns.iter().map(|p| p.to_string()).collect();
            assert_eq!(asks_for_appearance(&patterns), *expected, "{patterns:?}");
        }
    }
}

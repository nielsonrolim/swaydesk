//! Conserta o chaveiro criado no primeiro login, quando o daemon não o publica no D-Bus. O porte do
//! `sway-keyring` em Python, que explica o problema.
//!
//! No primeiro login sem desktop (greetd), o gnome-keyring-daemon cria o chaveiro "login" e não exporta a
//! coleção nova: ela aparece em Collections e o objeto não existe (a issue #137 do gnome-keyring no GNOME GitLab). Um daemon
//! novo lê o arquivo e a exporta, mas trancada: a senha que o PAM deu ao primeiro se perde. Então, em vez
//! de esperar o primeiro app pedir, o pedido de senha abre aqui, logo no login: uma vez, e os logins
//! seguintes destravam sozinhos (o Debian 13, com o gnome-keyring 48, tem o problema; o Ubuntu 26.04 e o
//! Arch, não). Uma notificação antes do pedido explica por que ele aparece. Sem o problema, não faz nada.
//! A correção upstream (o MR !99, "Register the login collection on dbus on creation") saiu no gnome-keyring
//! 51.0: com ele em todas as distros, este programa pode sair.
//! --check: só confere que o binário carrega.
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use gio::glib;
use gio::prelude::*;
use swaydesk_common::i18n::tr;
use swaydesk_common::resident;

const PROGRAM: &str = "sway-keyring";
const BUS_NAME: &str = "org.freedesktop.secrets";
const SERVICE: &str = "/org/freedesktop/secrets";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const UNIT: &str = "gnome-keyring-daemon.service";
const TIMEOUT_MS: i32 = 5000;
/// Quanto esperar o daemon novo publicar as coleções
const EXPORT_WAIT: Duration = Duration::from_secs(10);
const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
/// Quanto esperar o servidor de notificações aparecer antes do aviso
const NOTIFY_WAIT: Duration = Duration::from_secs(5);
/// Quanto esperar a resposta do servidor de notificações (só para o log: o pedido de senha não espera)
const NOTIFY_REPLY_MS: i32 = 60_000;
/// Quanto o aviso do primeiro login fica na tela: enquanto a pessoa lê o pedido de senha
const NOTIFY_TIMEOUT_MS: i32 = 30000;
/// Quanto o pedido de senha fica aberto: sair antes o cancelaria (o daemon cancela o de quem saiu)
const PROMPT_WAIT: Duration = Duration::from_secs(600);

/// As coleções listadas que não existem no D-Bus. `exported(path)` diz se o objeto existe; um erro
/// interrompe tudo (o Python também: um GLib.Error no meio sai sem consertar).
fn missing<E>(paths: &[String], mut exported: impl FnMut(&str) -> Result<bool, E>) -> Result<Vec<String>, E> {
    let mut broken = Vec::new();
    for path in paths {
        if !exported(path)? {
            broken.push(path.clone());
        }
    }
    Ok(broken)
}

fn call(bus: &gio::DBusConnection, path: &str, iface: &str, method: &str, params: Option<&glib::Variant>,
        reply: &str) -> Result<glib::Variant, glib::Error> {
    bus.call_sync(Some(BUS_NAME), path, iface, method, params, Some(glib::VariantTy::new(reply).expect("reply type")),
                  gio::DBusCallFlags::NO_AUTO_START, TIMEOUT_MS, gio::Cancellable::NONE)
}

fn collections(bus: &gio::DBusConnection) -> Result<Vec<String>, glib::Error> {
    let params = ("org.freedesktop.Secret.Service", "Collections").to_variant();
    let reply = call(bus, SERVICE, "org.freedesktop.DBus.Properties", "Get", Some(&params), "(v)")?;
    let value = reply.child_value(0).as_variant().expect("(v) holds a variant");
    // Um serviço de outro tipo pode responder outra coisa: sem a lista, nada a consertar
    if value.type_() != glib::VariantTy::OBJECT_PATH_ARRAY {
        return Ok(Vec::new());
    }
    Ok((0..value.n_children()).filter_map(|i| value.child_value(i).str().map(str::to_owned)).collect())
}

fn exported(bus: &gio::DBusConnection, path: &str) -> Result<bool, glib::Error> {
    let reply = call(bus, path, "org.freedesktop.DBus.Introspectable", "Introspect", None, "(s)")?;
    let xml = reply.child_value(0).str().unwrap_or_default().to_owned();
    let node = gio::DBusNodeInfo::for_xml(&xml)?;
    Ok(node.lookup_interface(COLLECTION).is_some())
}

/// Depois do restart: espera as coleções aparecerem e pede a senha das que estão trancadas. Fica até a
/// resposta (ou o prazo). Um erro aqui não muda o resultado do conserto: o chaveiro já existe.
fn unlock_with_prompt(paths: &[String]) {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    let start = Instant::now();
    while !paths.iter().all(|p| exported(&bus, p).unwrap_or(false)) {
        if start.elapsed() > EXPORT_WAIT {
            eprintln!("{PROGRAM}: the restarted daemon did not publish the collections");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let objects: Vec<glib::Variant> =
        paths.iter().filter_map(|p| glib::variant::ObjectPath::try_from(p.as_str()).ok()).map(|p| p.to_variant()).collect();
    let array = glib::Variant::array_from_iter_with_type(glib::VariantTy::OBJECT_PATH, objects);
    let params = glib::Variant::tuple_from_iter([array]);
    let reply = match call(&bus, SERVICE, "org.freedesktop.Secret.Service", "Unlock", Some(&params), "(aoo)") {
        Ok(reply) => reply,
        Err(err) => {
            eprintln!("{PROGRAM}: Unlock: {err}");
            return;
        }
    };
    let prompt = reply.child_value(1).str().unwrap_or("/").to_owned();
    if prompt == "/" {
        return; // já destravado
    }
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let _completed = bus.signal_subscribe(Some(BUS_NAME), Some("org.freedesktop.Secret.Prompt"), Some("Completed"), Some(&prompt), None,
                                          gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
        let dismissed = params.child_value(0).get::<bool>().unwrap_or(true);
        eprintln!("{PROGRAM}: {}", if dismissed { "the unlock prompt was dismissed" } else { "the login keyring is unlocked" });
        done.quit();
    });
    eprintln!("{PROGRAM}: asking for the password once to unlock the new login keyring");
    notify_first_login(&bus);
    if let Err(err) = call(&bus, &prompt, "org.freedesktop.Secret.Prompt", "Prompt", Some(&("",).to_variant()), "()") {
        eprintln!("{PROGRAM}: Prompt: {err}");
        return;
    }
    let timeout = main_loop.clone();
    glib::timeout_add_local_once(PROMPT_WAIT, move || timeout.quit());
    main_loop.run();
}

/// O nome tem dono no barramento?
fn has_owner(bus: &gio::DBusConnection, name: &str) -> bool {
    bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner",
                  Some(&(name,).to_variant()), Some(glib::VariantTy::new("(b)").expect("reply type")),
                  gio::DBusCallFlags::NONE, TIMEOUT_MS, gio::Cancellable::NONE)
        .ok()
        .and_then(|reply| reply.child_value(0).get::<bool>())
        .unwrap_or(false)
}

/// Explica o pedido de senha que vem a seguir (o texto do gcr não diz por que ele aparece). Sem servidor de
/// notificações, segue sem o aviso.
fn notify_first_login(bus: &gio::DBusConnection) {
    // No primeiro login, o servidor de notificações pode ainda estar subindo (ele espera o ambiente da
    // sessão): sem a espera, o aviso se perderia justo no único caso em que ele importa
    let start = Instant::now();
    while !has_owner(bus, NOTIFICATIONS) && start.elapsed() < NOTIFY_WAIT {
        std::thread::sleep(Duration::from_millis(500));
    }
    let summary = tr("Keyring");
    // TRANSLATORS: shown right before the password prompt, only on the first login on Debian 13
    let body = tr("First login: type your password once to turn on the keyring. From the next login on, it opens by itself.");
    let actions: Vec<String> = Vec::new();
    let hints: std::collections::HashMap<String, glib::Variant> = std::collections::HashMap::new();
    let params = ("Swaydesk", 0u32, "dialog-password", summary.as_str(), body.as_str(), actions, hints, NOTIFY_TIMEOUT_MS)
        .to_variant();
    // Sem esperar a resposta: o pedido de senha abre em seguida, e a resposta chega no laço dele. Num host
    // carregado, o servidor mostra o aviso e só responde segundos depois: com a chamada síncrona de 5 s, o log
    // dizia que o aviso falhou, e ele tinha aparecido
    bus.call(Some(NOTIFICATIONS), "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify",
             Some(&params), Some(glib::VariantTy::new("(u)").expect("reply type")), gio::DBusCallFlags::NONE,
             NOTIFY_REPLY_MS, gio::Cancellable::NONE, |reply| {
                 if let Err(err) = reply {
                     eprintln!("{PROGRAM}: the notification server did not take the notice: {err}");
                 }
             });
}

fn systemctl(args: &[&str]) -> Option<i32> {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("{PROGRAM}: could not run systemctl: {err}");
            None
        }
    }
}

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|a| a == "--check") {
        return ExitCode::SUCCESS;
    }
    resident::set_process_name(PROGRAM);
    let broken = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .and_then(|bus| missing(&collections(&bus)?, |path| exported(&bus, path)));
    let broken = match broken {
        Ok(broken) if !broken.is_empty() => broken,
        _ => return ExitCode::SUCCESS, // sem chaveiro rodando, ou de outro tipo: nada a consertar
    };
    eprintln!("{PROGRAM}: collections without a D-Bus object ({}): restarting {UNIT}", broken.join(", "));
    // Só o daemon do systemd: um iniciado de outro jeito não é nosso para reiniciar
    if systemctl(&["is-active", "--quiet", UNIT]) != Some(0) {
        eprintln!("{PROGRAM}: {UNIT} is not active; nothing was done");
        return ExitCode::FAILURE;
    }
    match systemctl(&["restart", UNIT]) {
        Some(0) => {
            unlock_with_prompt(&broken);
            ExitCode::SUCCESS
        }
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_collections_without_object() {
        let paths = vec!["/a".to_owned(), "/b".to_owned()];
        assert_eq!(missing::<()>(&paths, |p| Ok(p == "/b")), Ok(vec!["/a".to_owned()]));
        assert_eq!(missing::<()>(&paths[..1], |_| Ok(true)), Ok(vec![]));
        assert_eq!(missing(&paths, |_| Err("erro")), Err("erro"));
    }
}

//! O destaque de cada conta no AccountsService, o que a tela de login (swaydesk-greeter) lê antes de qualquer
//! login. A interface `org.swaydesktop.Accounts` vem do pacote swaydesk-common
//! (/usr/share/accountsservice/interfaces); o accounts-daemon guarda o valor em /var/lib/AccountsService/users e o
//! polkit deixa cada um mudar só a própria conta (change-own-user-data), sem senha. O mesmo do
//! `set_account_accent` do desktop_config.py.
use std::time::{Duration, Instant};

use gio::glib::{Variant, VariantTy};
use gio::prelude::*;

use crate::config::is_accent;

pub const ACCOUNTS: &str = "org.freedesktop.Accounts";
pub const ACCOUNTS_PATH: &str = "/org/freedesktop/Accounts";
pub const IFACE: &str = "org.swaydesktop.Accounts";
pub const PROPERTY: &str = "Accent";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// O prazo da conversa inteira (não de cada chamada): o setup roda a cada login antes do Sway, e um
/// accounts-daemon lento para subir no boot não pode atrasar a entrada.
const DEADLINE: Duration = Duration::from_secs(1);

/// Uma chamada com o que sobra do prazo. NONE de propósito, sem o ALLOW_INTERACTIVE_AUTHORIZATION: o polkit nunca
/// pede senha por isto; se a conta precisasse de autorização, a chamada só falha e fica por isso mesmo.
fn call(bus: &gio::DBusConnection, deadline: Instant, path: &str, iface: &str, method: &str, args: Variant, reply: Option<&VariantTy>) -> Option<Variant> {
    let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    let left = i32::try_from(left.as_millis()).unwrap_or(i32::MAX).max(1);
    bus.call_sync(Some(ACCOUNTS), path, iface, method, Some(&args), reply, gio::DBusCallFlags::NONE, left, gio::Cancellable::NONE)
        .ok()
}

fn system_bus() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE).ok()
}

fn accent_of(bus: &gio::DBusConnection, deadline: Instant, user: &str) -> Option<String> {
    let reply = call(bus, deadline, user, PROPERTIES, "Get", (IFACE, PROPERTY).to_variant(), Some(VariantTy::new("(v)").unwrap()))?;
    reply.child_value(0).as_variant()?.get::<String>()
}

/// Grava o destaque na conta de quem roda, se mudou. Sem o AccountsService, sem a interface (os dotfiles, um
/// contêiner) ou com a gravação recusada, nada: a tela de login fica com a cor dela. Devolve se gravou.
pub fn set_own_accent(accent: &str) -> bool {
    let deadline = Instant::now() + DEADLINE;
    let Some(bus) = system_bus() else { return false };
    // SAFETY: getuid não falha nem mexe em memória
    let uid = unsafe { libc::getuid() } as i64;
    let Some(user) = call(&bus, deadline, ACCOUNTS_PATH, ACCOUNTS, "FindUserById", (uid,).to_variant(), Some(VariantTy::new("(o)").unwrap()))
        .and_then(|v| v.child_value(0).str().map(str::to_string))
    else {
        return false;
    };
    if accent_of(&bus, deadline, &user).as_deref() == Some(accent) {
        return false;
    }
    call(&bus, deadline, &user, PROPERTIES, "Set", (IFACE, PROPERTY, accent.to_variant()).to_variant(), None).is_some()
}

/// O destaque gravado na conta `login`, se for uma cor válida (`#rrggbb`): o valor vem de cada usuário, e a tela
/// de login só põe no CSS o que passa pelo is_accent.
pub fn user_accent(login: &str) -> Option<String> {
    let deadline = Instant::now() + DEADLINE;
    let bus = system_bus()?;
    let user = call(&bus, deadline, ACCOUNTS_PATH, ACCOUNTS, "FindUserByName", (login,).to_variant(), Some(VariantTy::new("(o)").unwrap()))?
        .child_value(0)
        .str()?
        .to_string();
    accent_of(&bus, deadline, &user).filter(|accent| is_accent(accent))
}

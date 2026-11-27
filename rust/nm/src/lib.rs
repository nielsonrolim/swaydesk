//! O estado do NetworkManager pelo D-Bus, sem GTK: o que a libnm dava ao `waybar-network` e ao
//! `waybar-network-popup` em Python. Uma leitura é um [`Snapshot`]: o `GetManagedObjects` do NM (as
//! propriedades de todos os objetos) e o `GetSettings` de cada perfil salvo.
//!
//! Sem o NM no barramento, o [`standalone`] lê o Wi-Fi do iwd e o cabo do systemd-networkd no mesmo
//! [`Snapshot`]; o [`Snapshot::backend`] diz de onde veio.
//!
//! SENHAS: nada aqui lê segredos. O `GetSettings` não traz senhas (elas só vêm pelo `GetSecrets`, que
//! este código não chama), e a ativação deixa o NM perguntar ao agente de segredos (o nm-applet).
mod rules;
pub mod standalone;

use std::collections::HashMap;

use gio::glib::{self, prelude::*, Variant};

pub use rules::*;

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const IFACE_DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const IFACE_WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const IFACE_WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
const IFACE_AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const IFACE_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IFACE_IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
const IFACE_IP6: &str = "org.freedesktop.NetworkManager.IP6Config";
const CALL_MS: i32 = 5000;

/// Os estados do NMDeviceState que importam aqui.
pub mod device_state {
    pub const UNMANAGED: u32 = 10;
    pub const UNAVAILABLE: u32 = 20;
    pub const DISCONNECTED: u32 = 30;
    pub const NEED_AUTH: u32 = 60;
    pub const ACTIVATED: u32 = 100;
    /// PREPARE, CONFIG, IP_CONFIG, IP_CHECK, SECONDARIES
    pub const CONNECTING: [u32; 5] = [40, 50, 70, 80, 90];
}

/// NMActiveConnectionState e o motivo de quem desligou.
pub mod active_state {
    pub const ACTIVATING: u32 = 1;
    pub const ACTIVATED: u32 = 2;
    pub const DEACTIVATED: u32 = 4;
    pub const REASON_USER_DISCONNECTED: u32 = 2;
}

/// NMConnectivityState
pub mod connectivity {
    pub const NONE: u32 = 1;
    pub const PORTAL: u32 = 2;
    pub const LIMITED: u32 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Wifi,
    /// Ethernet e as subclasses do NM.DeviceEthernet (veth)
    Ethernet,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessPoint {
    pub path: String,
    pub ssid: String,
    pub strength: u8,
    pub flags: u32,
    pub wpa: u32,
    pub rsn: u32,
    pub frequency: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub path: String,
    pub iface: String,
    pub kind: DeviceKind,
    pub state: u32,
    pub hw_address: String,
    pub active_connection: Option<String>,
    pub access_points: Vec<String>,
    pub active_access_point: Option<String>,
    /// Kb/s (Wi-Fi)
    pub bitrate: u32,
    /// Mb/s (cabo)
    pub speed: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Active {
    pub path: String,
    pub id: String,
    pub uuid: String,
    pub kind: String,
    pub state: u32,
    pub devices: Vec<String>,
    pub ip4: Option<String>,
    pub ip6: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IpConfig {
    /// "endereço/prefixo"
    pub addresses: Vec<String>,
    pub gateway: String,
    pub dns: Vec<String>,
}

/// Um perfil salvo: só o que a lista usa (nada de segredos).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Connection {
    pub path: String,
    pub id: String,
    pub uuid: String,
    pub kind: String,
    /// O SSID de um perfil Wi-Fi
    pub ssid: Option<String>,
    pub mode: String,
    pub iface: String,
    pub vpn_service: String,
}

/// De onde veio o estado.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Backend {
    /// Nenhum serviço de rede no barramento
    #[default]
    None,
    NetworkManager,
    /// Sem o NM: o Wi-Fi pelo iwd e o cabo pelo systemd-networkd (um dos dois, ou os dois). Não há VPN,
    /// perfis para editar nem o interruptor da rede; a senha de uma rede nova vem pelo agente do popup
    Standalone,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub backend: Backend,
    /// Algum serviço de rede respondeu (o NM, ou o iwd ou o networkd)
    pub running: bool,
    /// O iwd está no ar, mas recusou a leitura (o usuário fora dos grupos da política dele)
    pub iwd_denied: bool,
    pub networking: bool,
    pub wireless: bool,
    pub wireless_hardware: bool,
    /// Alguma placa de Wi-Fi vista fora de `devices` (sem o NM): um adaptador do iwd, mesmo sem Device
    /// (desligado pelo rfkill), ou uma interface wlan do networkd (sem o iwd). Ver [`wifi_absent`]
    pub wifi_adapter: bool,
    /// O NM ainda está subindo (Startup), e a lista de placas pode estar incompleta
    pub starting: bool,
    pub connectivity: u32,
    pub primary: Option<String>,
    pub activating: Option<String>,
    pub devices: Vec<Device>,
    pub access_points: HashMap<String, AccessPoint>,
    pub actives: Vec<Active>,
    pub ip: HashMap<String, IpConfig>,
    pub connections: Vec<Connection>,
}

impl Snapshot {
    pub fn active(&self, path: Option<&str>) -> Option<&Active> {
        path.and_then(|p| self.actives.iter().find(|a| a.path == p))
    }

    pub fn access_point(&self, path: Option<&str>) -> Option<&AccessPoint> {
        path.and_then(|p| self.access_points.get(p))
    }

    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.path == path)
    }
}

/// O SSID em texto, como o `nm_utils_ssid_to_utf8`: UTF-8 se for válido; senão, Latin-1 (o NM tenta
/// as codificações da língua e cai nela).
pub fn ssid_to_utf8(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(text) => text.to_string(),
        Err(_) => raw.iter().map(|b| char::from(*b)).collect(),
    }
}

pub(crate) fn path_or_none(v: Option<Variant>) -> Option<String> {
    v.and_then(|v| v.str().map(String::from)).filter(|p| p != "/")
}

fn paths(v: Option<Variant>) -> Vec<String> {
    let Some(v) = v else { return Vec::new() };
    (0..v.n_children()).filter_map(|i| v.child_value(i).str().map(String::from)).collect()
}

/// As propriedades a{sv} de uma interface, desembrulhadas.
pub(crate) type Props = HashMap<String, Variant>;

pub(crate) fn props_of(dict: &Variant) -> Props {
    let mut props = Props::new();
    for i in 0..dict.n_children() {
        let entry = dict.child_value(i);
        if let (Some(key), Some(value)) = (entry.child_value(0).str().map(String::from), entry.child_value(1).as_variant()) {
            props.insert(key, value);
        }
    }
    props
}

pub(crate) fn get<T: glib::variant::FromVariant>(props: &Props, key: &str) -> Option<T> {
    props.get(key).and_then(|v| v.get::<T>())
}

pub(crate) fn text(props: &Props, key: &str) -> String {
    props.get(key).and_then(|v| v.str().map(String::from)).unwrap_or_default()
}

/// Um valor de um a{sv}, desembrulhado.
fn lookup(dict: &Variant, key: &str) -> Option<Variant> {
    (0..dict.n_children()).map(|i| dict.child_value(i)).find(|e| e.child_value(0).str() == Some(key)).and_then(|e| e.child_value(1).as_variant())
}

fn address_data(props: &Props) -> Vec<(String, Option<u32>)> {
    let Some(list) = props.get("AddressData") else { return Vec::new() };
    (0..list.n_children())
        .filter_map(|i| {
            let entry = list.child_value(i);
            let address = lookup(&entry, "address")?.str()?.to_string();
            Some((address, lookup(&entry, "prefix").and_then(|p| p.get::<u32>())))
        })
        .collect()
}

fn ip_config(props: &Props) -> IpConfig {
    let addresses = address_data(props).into_iter().map(|(a, p)| format!("{a}/{}", p.unwrap_or(0))).collect();
    let dns = props
        .get("NameserverData")
        .map(|list| {
            (0..list.n_children())
                .filter_map(|i| lookup(&list.child_value(i), "address").and_then(|a| a.str().map(String::from)))
                .collect()
        })
        .unwrap_or_default();
    IpConfig { addresses, gateway: text(props, "Gateway"), dns }
}

fn call(bus: &gio::DBusConnection, path: &str, iface: &str, method: &str, args: Option<&Variant>) -> Result<Variant, glib::Error> {
    call_to(bus, NM, path, iface, method, args)
}

/// Sem ativar o serviço pelo D-Bus: o chamador já conferiu que o nome tem dono.
pub(crate) fn call_to(bus: &gio::DBusConnection, dest: &str, path: &str, iface: &str, method: &str, args: Option<&Variant>) -> Result<Variant, glib::Error> {
    bus.call_sync(Some(dest), path, iface, method, args, None, gio::DBusCallFlags::NO_AUTO_START, CALL_MS, gio::Cancellable::NONE)
}

/// Se o nome tem dono agora. Pergunta antes de chamar: o NM, o iwd e o networkd vêm com arquivo de
/// ativação do D-Bus, e uma chamada sem dono ligaria um serviço que o usuário deixou desligado.
pub fn has_owner(bus: &gio::DBusConnection, name: &str) -> bool {
    bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", Some(&(name,).to_variant()),
        Some(glib::VariantTy::new("(b)").expect("tipo")), gio::DBusCallFlags::NONE, CALL_MS, gio::Cancellable::NONE)
        .ok()
        .and_then(|v| v.get::<(bool,)>())
        .is_some_and(|(owned,)| owned)
}

/// Os objetos de um GetManagedObjects: caminho → interface → propriedades.
pub(crate) type Objects = HashMap<String, HashMap<String, Props>>;

pub(crate) fn managed_objects(objects: &Variant) -> Objects {
    let mut by_path = Objects::new();
    for i in 0..objects.n_children() {
        let entry = objects.child_value(i);
        let Some(path) = entry.child_value(0).str().map(String::from) else { continue };
        let ifaces = entry.child_value(1);
        let mut found = HashMap::new();
        for j in 0..ifaces.n_children() {
            let iface = ifaces.child_value(j);
            if let Some(name) = iface.child_value(0).str() {
                found.insert(name.to_string(), props_of(&iface.child_value(1)));
            }
        }
        by_path.insert(path, found);
    }
    by_path
}

/// Os campos de um perfil salvo, do `GetSettings` (que não traz senhas).
fn connection_of(path: &str, settings: &Variant) -> Connection {
    let mut conn = Connection { path: path.to_string(), ..Default::default() };
    for i in 0..settings.n_children() {
        let group = settings.child_value(i);
        let (Some(name), dict) = (group.child_value(0).str().map(String::from), group.child_value(1)) else { continue };
        let props = props_of(&dict);
        match name.as_str() {
            "connection" => {
                conn.id = text(&props, "id");
                conn.uuid = text(&props, "uuid");
                conn.kind = text(&props, "type");
                conn.iface = text(&props, "interface-name");
            }
            "802-11-wireless" => {
                conn.ssid = get::<Vec<u8>>(&props, "ssid").map(|raw| ssid_to_utf8(&raw));
                conn.mode = text(&props, "mode");
            }
            "vpn" => conn.vpn_service = text(&props, "service-type"),
            _ => {}
        }
    }
    conn
}

/// Lê o estado inteiro; com `settings`, também os perfis salvos (um `GetSettings` por perfil: o ícone
/// não precisa deles). Síncrono: chame fora da linha da interface. O NM no barramento vale mais (com o
/// iwd por baixo dele também); sem ele, o iwd e o networkd ([`standalone`]). Sem nenhum, `running` fica
/// falso e o resto vazio.
pub fn read(bus: &gio::DBusConnection, settings: bool) -> Snapshot {
    if has_owner(bus, NM) {
        read_nm(bus, settings)
    } else {
        standalone::read(bus)
    }
}

fn read_nm(bus: &gio::DBusConnection, settings: bool) -> Snapshot {
    let mut snap = Snapshot::default();
    let Ok(objects) = call(bus, "/org/freedesktop", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", None) else {
        return snap;
    };
    snap.running = true;
    snap.backend = Backend::NetworkManager;
    let by_path = managed_objects(&objects.child_value(0));
    let empty = HashMap::new();
    let manager = by_path.get(NM_PATH).and_then(|m| m.get(NM)).unwrap_or(&empty);
    snap.networking = get(manager, "NetworkingEnabled").unwrap_or(false);
    snap.wireless = get(manager, "WirelessEnabled").unwrap_or(false);
    snap.wireless_hardware = get(manager, "WirelessHardwareEnabled").unwrap_or(false);
    // Sem o objeto do gerenciador, também não se sabe que placas há
    snap.starting = get(manager, "Startup").unwrap_or(true);
    snap.connectivity = get(manager, "Connectivity").unwrap_or(0);
    snap.primary = path_or_none(manager.get("PrimaryConnection").cloned());
    snap.activating = path_or_none(manager.get("ActivatingConnection").cloned());
    for path in paths(manager.get("Devices").cloned()) {
        let Some(ifaces) = by_path.get(&path) else { continue };
        let Some(dev) = ifaces.get(IFACE_DEVICE) else { continue };
        let (wireless, wired) = (ifaces.get(IFACE_WIRELESS), ifaces.get(IFACE_WIRED));
        let device_type: u32 = get(dev, "DeviceType").unwrap_or(0);
        let kind = match (device_type, wireless.is_some(), wired.is_some()) {
            (2, _, _) | (_, true, _) => DeviceKind::Wifi,
            (1 | 20, _, _) | (_, _, true) => DeviceKind::Ethernet,
            _ => DeviceKind::Other,
        };
        let extra = wireless.or(wired);
        snap.devices.push(Device {
            iface: text(dev, "Interface"),
            kind,
            state: get(dev, "State").unwrap_or(0),
            hw_address: extra.map(|e| text(e, "HwAddress")).filter(|h| !h.is_empty()).unwrap_or_else(|| text(dev, "HwAddress")),
            active_connection: path_or_none(dev.get("ActiveConnection").cloned()),
            access_points: wireless.map(|w| paths(w.get("AccessPoints").cloned())).unwrap_or_default(),
            active_access_point: wireless.and_then(|w| path_or_none(w.get("ActiveAccessPoint").cloned())),
            bitrate: wireless.and_then(|w| get(w, "Bitrate")).unwrap_or(0),
            speed: wired.and_then(|w| get(w, "Speed")).unwrap_or(0),
            path,
        });
    }
    for (path, ifaces) in &by_path {
        if let Some(ap) = ifaces.get(IFACE_AP) {
            snap.access_points.insert(path.clone(), AccessPoint {
                path: path.clone(),
                ssid: get::<Vec<u8>>(ap, "Ssid").map(|raw| ssid_to_utf8(&raw)).unwrap_or_default(),
                strength: get(ap, "Strength").unwrap_or(0),
                flags: get(ap, "Flags").unwrap_or(0),
                wpa: get(ap, "WpaFlags").unwrap_or(0),
                rsn: get(ap, "RsnFlags").unwrap_or(0),
                frequency: get(ap, "Frequency").unwrap_or(0),
            });
        }
        if let Some(cfg) = ifaces.get(IFACE_IP4).or_else(|| ifaces.get(IFACE_IP6)) {
            snap.ip.insert(path.clone(), ip_config(cfg));
        }
    }
    for path in paths(manager.get("ActiveConnections").cloned()) {
        let Some(a) = by_path.get(&path).and_then(|i| i.get(IFACE_ACTIVE)) else { continue };
        snap.actives.push(Active {
            id: text(a, "Id"),
            uuid: text(a, "Uuid"),
            kind: text(a, "Type"),
            state: get(a, "State").unwrap_or(0),
            devices: paths(a.get("Devices").cloned()),
            ip4: path_or_none(a.get("Ip4Config").cloned()),
            ip6: path_or_none(a.get("Ip6Config").cloned()),
            path,
        });
    }
    // Os perfis na ordem do NM; um que some entre a lista e o GetSettings fica de fora
    let settings_path = "/org/freedesktop/NetworkManager/Settings";
    let listed = by_path.get(settings_path).and_then(|s| s.get("org.freedesktop.NetworkManager.Settings")).map(|s| paths(s.get("Connections").cloned()));
    for path in listed.filter(|_| settings).unwrap_or_default() {
        if let Ok(settings) = call(bus, &path, "org.freedesktop.NetworkManager.Settings.Connection", "GetSettings", None) {
            snap.connections.push(connection_of(&path, &settings.child_value(0)));
        }
    }
    snap
}

/// Assina tudo o que o NM, o iwd e o networkd avisam (propriedades, objetos novos e removidos, perfis,
/// estados) e a troca de dono dos três nomes, que troca o backend: cada aviso chama `changed`, que agrupa e
/// relê. Devolve as assinaturas, para manter vivas.
#[allow(deprecated)] // o signal_subscribe com closure continua o caminho simples no gio 0.20
pub fn watch(bus: &gio::DBusConnection, changed: impl Fn() + Clone + 'static) -> Vec<gio::SignalSubscriptionId> {
    let mut subs = Vec::new();
    for name in [NM, standalone::IWD, standalone::NETWORKD] {
        let on_any = changed.clone();
        subs.push(bus.signal_subscribe(Some(name), None, None, None, None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| on_any()));
        let on_owner = changed.clone();
        subs.push(bus.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(name),
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, _| on_owner(),
        ));
    }
    subs
}

pub fn system_bus() -> Result<gio::DBusConnection, glib::Error> {
    gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
}

#[cfg(test)]
mod tests;

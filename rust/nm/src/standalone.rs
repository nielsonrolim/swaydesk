//! O estado sem o NetworkManager: o Wi-Fi pelo iwd (`net.connman.iwd`) e o cabo pelo systemd-networkd
//! (`org.freedesktop.network1`), no mesmo [`Snapshot`] do NM, para as regras do ícone e do popup valerem
//! sem mudança. O que não existe nesses serviços (VPN, perfis editáveis, o interruptor da rede, a
//! conectividade) fica vazio.
//!
//! O mapa: cada Device do iwd no modo station é um [`Device`] Wi-Fi; cada Network do GetOrderedNetworks
//! é um [`AccessPoint`] (o caminho dele serve para o `Network.Connect`); a rede conectada ou conectando é
//! um [`Active`]; cada KnownNetwork é um [`Connection`] salvo. Cada link "ether" que o networkd gerencia
//! é um [`Device`] cabeado. Os endereços vêm do kernel (getifaddrs), e o gateway e o DNS vêm do networkd,
//! quando ele está no ar.
//!
//! Os nomes das redes vêm de fora (qualquer um cria um SSID): quem mostra faz o escape.
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use gio::glib::Variant;
use serde_json::Value;

use crate::{
    active_state, call_to, device_state, get, has_owner, managed_objects, path_or_none, text, AccessPoint, Active, Backend, Connection, Device,
    DeviceKind, IpConfig, Objects, Props, Snapshot,
};

pub const IWD: &str = "net.connman.iwd";
pub const NETWORKD: &str = "org.freedesktop.network1";
pub const NETWORKD_PATH: &str = "/org/freedesktop/network1";
pub const IWD_ADAPTER: &str = "net.connman.iwd.Adapter";
pub const IWD_DEVICE: &str = "net.connman.iwd.Device";
pub const IWD_STATION: &str = "net.connman.iwd.Station";
pub const IWD_NETWORK: &str = "net.connman.iwd.Network";
pub const IWD_KNOWN: &str = "net.connman.iwd.KnownNetwork";
pub const IWD_DIAGNOSTIC: &str = "net.connman.iwd.StationDiagnostic";
pub const IWD_AGENT_MANAGER: &str = "net.connman.iwd.AgentManager";
pub const IWD_AGENT_MANAGER_PATH: &str = "/net/connman/iwd";
pub const IWD_AGENT: &str = "net.connman.iwd.Agent";
pub const WIRED_TYPE: &str = "802-3-ethernet";
/// O prefixo dos caminhos dos links cabeados no Snapshot (o networkd não é chamado por eles)
pub const WIRED_PREFIX: &str = "networkd:";

// Os mesmos bits do NM que o security_of lê
const AP_PRIVACY: u32 = 0x1;
const SEC_PSK: u32 = 0x100;
const SEC_8021X: u32 = 0x200;

/// O que se leu dos serviços, antes de virar Snapshot (separado para os testes).
#[derive(Debug, Default)]
pub struct Raw {
    /// O GetManagedObjects do iwd; None com ele fora do ar
    pub iwd: Option<Objects>,
    pub iwd_denied: bool,
    /// Station → as redes do GetOrderedNetworks, na ordem dele: (caminho, sinal em 100 × dBm)
    pub ordered: HashMap<String, Vec<(String, i16)>>,
    /// Station → o GetDiagnostics (só existe conectada)
    pub diagnostics: HashMap<String, Props>,
    /// O Manager.Describe do networkd (JSON); None com ele fora do ar
    pub networkd: Option<Value>,
    /// Interface → (IPv4, IPv6), "endereço/prefixo"
    pub addresses: HashMap<String, (Vec<String>, Vec<String>)>,
    /// As rotas padrão IPv4 do kernel: (interface, métrica)
    pub default_routes: Vec<(String, u32)>,
}

/// As rotas padrão do `/proc/net/route` (destino e máscara zerados), com a métrica.
pub fn default_routes(table: &str) -> Vec<(String, u32)> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (iface, dest, metric, mask) = (fields.first()?, fields.get(1)?, fields.get(6)?, fields.get(7)?);
            (*dest == "00000000" && *mask == "00000000").then(|| Some((iface.to_string(), metric.parse().ok()?)))?
        })
        .collect()
}

/// A força em %, do sinal em 100 × dBm: -50 dBm ou mais é 100%, -100 dBm é 0%.
pub fn strength_of(signal: i16) -> u8 {
    let dbm = i32::from(signal) / 100;
    ((dbm + 100) * 2).clamp(0, 100) as u8
}

/// Um endereço do JSON do networkd: uma lista de bytes (4 ou 16).
fn ip_text(value: &Value) -> Option<String> {
    let bytes: Vec<u8> = value.as_array()?.iter().map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok())).collect::<Option<_>>()?;
    match bytes.len() {
        4 => Some(Ipv4Addr::from(<[u8; 4]>::try_from(bytes).ok()?).to_string()),
        16 => Some(Ipv6Addr::from(<[u8; 16]>::try_from(bytes).ok()?).to_string()),
        _ => None,
    }
}

/// O MAC do JSON: uma lista de 6 bytes ou, em versões antigas, já em texto.
fn mac_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .map(|b| b.iter().filter_map(|b| b.as_u64()).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"))
        .unwrap_or_default()
}

fn link_of<'a>(networkd: Option<&'a Value>, iface: &str) -> Option<&'a Value> {
    networkd?.get("Interfaces")?.as_array()?.iter().find(|l| l.get("Name").and_then(Value::as_str) == Some(iface))
}

/// O gateway IPv4 (a rota padrão) e os DNS do link, do networkd.
fn route_and_dns(link: Option<&Value>, family: u64) -> (String, Vec<String>) {
    let Some(link) = link else { return (String::new(), Vec::new()) };
    let fam = |v: &Value| v.get("Family").and_then(Value::as_u64) == Some(family);
    let gateway = link
        .get("Routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| fam(r) && r.get("DestinationPrefixLength").and_then(Value::as_u64) == Some(0))
        .find_map(|r| r.get("Gateway").and_then(ip_text))
        .unwrap_or_default();
    let dns = link.get("DNS").and_then(Value::as_array).into_iter().flatten().filter(|d| fam(d)).filter_map(|d| d.get("Address").and_then(ip_text)).collect();
    (gateway, dns)
}

/// Os IPs do Active de uma interface: as chaves "ip4:<iface>" e "ip6:<iface>" no `snap.ip`.
fn add_ip(snap: &mut Snapshot, raw: &Raw, iface: &str, active: &mut Active) {
    let (v4, v6) = raw.addresses.get(iface).cloned().unwrap_or_default();
    let link = link_of(raw.networkd.as_ref(), iface);
    let (gateway, dns) = route_and_dns(link, 2);
    if !v4.is_empty() || !gateway.is_empty() {
        let key = format!("ip4:{iface}");
        snap.ip.insert(key.clone(), IpConfig { addresses: v4, gateway, dns });
        active.ip4 = Some(key);
    }
    let (gateway, dns) = route_and_dns(link, 10);
    if !v6.is_empty() || !gateway.is_empty() {
        let key = format!("ip6:{iface}");
        snap.ip.insert(key.clone(), IpConfig { addresses: v6, gateway, dns });
        active.ip6 = Some(key);
    }
}

fn wifi(snap: &mut Snapshot, raw: &Raw, objects: &Objects) {
    let mut paths: Vec<&String> = objects.keys().collect();
    paths.sort();
    for path in paths {
        let ifaces = &objects[path];
        if let Some(known) = ifaces.get(IWD_KNOWN) {
            let name = text(known, "Name");
            snap.connections.push(Connection {
                path: path.clone(),
                id: name.clone(),
                uuid: path.clone(),
                kind: crate::WIFI_TYPE.into(),
                ssid: Some(name),
                ..Default::default()
            });
        }
        let Some(dev) = ifaces.get(IWD_DEVICE) else { continue };
        let mode = text(dev, "Mode");
        if !mode.is_empty() && mode != "station" {
            continue; // ponto de acesso ou ad-hoc: não é uma placa para procurar redes
        }
        let iface = text(dev, "Name");
        let adapter = path_or_none(dev.get("Adapter").cloned()).and_then(|a| objects.get(&a)).and_then(|a| a.get(IWD_ADAPTER));
        snap.wireless |= get::<bool>(dev, "Powered").unwrap_or(false);
        snap.wireless_hardware |= adapter.and_then(|a| get::<bool>(a, "Powered")).unwrap_or(true);
        let station = ifaces.get(IWD_STATION);
        let st = station.map(|s| text(s, "State")).unwrap_or_default();
        let state = match (station, st.as_str()) {
            (None, _) => device_state::UNAVAILABLE,
            (_, "connected") => device_state::ACTIVATED,
            (_, "connecting" | "roaming") => device_state::CONNECTING[0],
            _ => device_state::DISCONNECTED,
        };
        let connected = station.and_then(|s| path_or_none(s.get("ConnectedNetwork").cloned()));
        let diag = raw.diagnostics.get(path);
        let mut aps = Vec::new();
        for (net_path, signal) in raw.ordered.get(path).into_iter().flatten() {
            let Some(net) = objects.get(net_path).and_then(|o| o.get(IWD_NETWORK)) else { continue };
            let (flags, rsn) = match text(net, "Type").as_str() {
                "psk" => (AP_PRIVACY, SEC_PSK),
                "8021x" => (AP_PRIVACY, SEC_8021X),
                "wep" => (AP_PRIVACY, 0),
                _ => (0, 0),
            };
            let frequency = if connected.as_ref() == Some(net_path) { diag.and_then(|d| get::<u32>(d, "Frequency")).unwrap_or(0) } else { 0 };
            snap.access_points.insert(net_path.clone(), AccessPoint {
                path: net_path.clone(),
                ssid: text(net, "Name"),
                strength: strength_of(*signal),
                flags,
                wpa: 0,
                rsn,
                frequency,
            });
            aps.push(net_path.clone());
        }
        let mut device = Device {
            path: path.clone(),
            iface: iface.clone(),
            kind: DeviceKind::Wifi,
            state,
            hw_address: text(dev, "Address"),
            active_connection: None,
            access_points: aps,
            active_access_point: None,
            // Kb/s, como o NM
            bitrate: diag.and_then(|d| get::<u32>(d, "TxBitrate")).unwrap_or(0),
            speed: 0,
        };
        // A rede conectada (ou conectando) que saiu do GetOrderedNetworks continua na lista
        if let Some(net_path) = connected.as_ref().filter(|p| !snap.access_points.contains_key(*p)) {
            if let Some(net) = objects.get(net_path).and_then(|o| o.get(IWD_NETWORK)) {
                let rssi = diag.and_then(|d| get::<i16>(d, "RSSI")).unwrap_or(-100);
                snap.access_points.insert(net_path.clone(), AccessPoint {
                    path: net_path.clone(),
                    ssid: text(net, "Name"),
                    strength: strength_of(rssi.saturating_mul(100)),
                    ..Default::default()
                });
                device.access_points.push(net_path.clone());
            }
        }
        if let Some(net_path) = connected.filter(|_| state == device_state::ACTIVATED || device_state::CONNECTING.contains(&state)) {
            let id = snap.access_points.get(&net_path).map(|a| a.ssid.clone()).unwrap_or_default();
            let mut active = Active {
                path: net_path.clone(),
                id,
                uuid: net_path.clone(),
                kind: crate::WIFI_TYPE.into(),
                state: if state == device_state::ACTIVATED { active_state::ACTIVATED } else { active_state::ACTIVATING },
                devices: vec![path.clone()],
                ..Default::default()
            };
            add_ip(snap, raw, &iface, &mut active);
            device.active_connection = Some(net_path.clone());
            device.active_access_point = Some(net_path);
            snap.actives.push(active);
        }
        snap.devices.push(device);
    }
}

fn wired(snap: &mut Snapshot, raw: &Raw) {
    let Some(links) = raw.networkd.as_ref().and_then(|d| d.get("Interfaces")).and_then(Value::as_array) else { return };
    for link in links {
        let field = |key: &str| link.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        if field("Type") != "ether" {
            continue;
        }
        let admin = field("AdministrativeState");
        if matches!(admin.as_str(), "unmanaged" | "") {
            continue; // de outro programa (uma ponte de VM, o Docker)
        }
        let iface = field("Name");
        let oper = field("OperationalState");
        let state = match (admin.as_str(), oper.as_str()) {
            (_, "off" | "no-carrier") => device_state::UNAVAILABLE,
            ("pending" | "configuring", _) => device_state::CONNECTING[0],
            ("configured", "routable" | "degraded" | "carrier") => device_state::ACTIVATED,
            _ => device_state::DISCONNECTED,
        };
        let path = format!("{WIRED_PREFIX}{iface}");
        let mut device = Device {
            path: path.clone(),
            iface: iface.clone(),
            kind: DeviceKind::Ethernet,
            state,
            hw_address: link.get("HardwareAddress").map(mac_text).unwrap_or_default(),
            active_connection: None,
            access_points: Vec::new(),
            active_access_point: None,
            bitrate: 0,
            speed: 0,
        };
        if state == device_state::ACTIVATED {
            let mut active = Active {
                path: path.clone(),
                id: iface.clone(),
                uuid: path.clone(),
                kind: WIRED_TYPE.into(),
                state: active_state::ACTIVATED,
                devices: vec![path.clone()],
                ..Default::default()
            };
            add_ip(snap, raw, &iface, &mut active);
            device.active_connection = Some(path);
            snap.actives.push(active);
        }
        snap.devices.push(device);
    }
}

/// O Snapshot a partir do que se leu. A conexão principal é a da rota padrão de métrica menor no kernel
/// (o iwd e o networkd põem métricas próprias, e o Wi-Fi pode ganhar do cabo); sem rota, o cabo e depois o
/// Wi-Fi.
pub fn build(raw: &Raw) -> Snapshot {
    let mut snap = Snapshot {
        backend: Backend::Standalone,
        running: raw.iwd.is_some() || raw.iwd_denied || raw.networkd.is_some(),
        iwd_denied: raw.iwd_denied,
        networking: true,
        ..Default::default()
    };
    if !snap.running {
        return Snapshot::default();
    }
    wired(&mut snap, raw);
    if let Some(objects) = &raw.iwd {
        wifi(&mut snap, raw, objects);
    }
    let first = |kind: &str, state: u32| snap.actives.iter().find(|a| a.kind == kind && a.state == state).map(|a| a.path.clone());
    let iface_of = |active: &Active| active.devices.first().and_then(|d| snap.device(d)).map(|d| d.iface.clone());
    let by_route = snap
        .actives
        .iter()
        .filter(|a| a.state == active_state::ACTIVATED)
        .filter_map(|a| {
            let iface = iface_of(a)?;
            let metric = raw.default_routes.iter().filter(|(i, _)| *i == iface).map(|(_, m)| *m).min()?;
            Some((metric, a.path.clone()))
        })
        .min()
        .map(|(_, path)| path);
    snap.primary = by_route.or_else(|| first(WIRED_TYPE, active_state::ACTIVATED)).or_else(|| first(crate::WIFI_TYPE, active_state::ACTIVATED));
    snap.activating = first(crate::WIFI_TYPE, active_state::ACTIVATING);
    snap
}

/// Os endereços de cada interface, do kernel.
fn interface_addresses() -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut out: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs preenche a lista, liberada com freeifaddrs no fim; cada nó só é lido aqui dentro
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut node = list;
        while !node.is_null() {
            let entry = &*node;
            node = entry.ifa_next;
            if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(entry.ifa_name).to_string_lossy().into_owned();
            let prefix = |mask: &[u8]| mask.iter().map(|b| b.count_ones()).sum::<u32>();
            match i32::from((*entry.ifa_addr).sa_family) {
                libc::AF_INET => {
                    let addr = &*(entry.ifa_addr as *const libc::sockaddr_in);
                    let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
                    let bits = if entry.ifa_netmask.is_null() { 0 } else { prefix(&(*(entry.ifa_netmask as *const libc::sockaddr_in)).sin_addr.s_addr.to_ne_bytes()) };
                    out.entry(name).or_default().0.push(format!("{ip}/{bits}"));
                }
                libc::AF_INET6 => {
                    let addr = &*(entry.ifa_addr as *const libc::sockaddr_in6);
                    let ip = Ipv6Addr::from(addr.sin6_addr.s6_addr);
                    let bits = if entry.ifa_netmask.is_null() { 0 } else { prefix(&(*(entry.ifa_netmask as *const libc::sockaddr_in6)).sin6_addr.s6_addr) };
                    out.entry(name).or_default().1.push(format!("{ip}/{bits}"));
                }
                _ => {}
            }
        }
        libc::freeifaddrs(list);
    }
    out
}

/// O GetOrderedNetworks: a(on).
fn ordered_networks(reply: &Variant) -> Vec<(String, i16)> {
    let list = reply.child_value(0);
    (0..list.n_children())
        .filter_map(|i| {
            let entry = list.child_value(i);
            Some((entry.child_value(0).str()?.to_string(), entry.child_value(1).get::<i16>()?))
        })
        .collect()
}

/// Lê o iwd e o networkd, os que tiverem dono no barramento. Síncrono: chame fora da linha da interface.
pub fn read(bus: &gio::DBusConnection) -> Snapshot {
    let mut raw = Raw::default();
    if has_owner(bus, IWD) {
        match call_to(bus, IWD, "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", None) {
            Ok(reply) => {
                let objects = managed_objects(&reply.child_value(0));
                for (path, ifaces) in &objects {
                    let Some(station) = ifaces.get(IWD_STATION) else { continue };
                    if let Ok(reply) = call_to(bus, IWD, path, IWD_STATION, "GetOrderedNetworks", None) {
                        raw.ordered.insert(path.clone(), ordered_networks(&reply));
                    }
                    if text(station, "State") == "connected" && ifaces.contains_key(IWD_DIAGNOSTIC) {
                        if let Ok(reply) = call_to(bus, IWD, path, IWD_DIAGNOSTIC, "GetDiagnostics", None) {
                            raw.diagnostics.insert(path.clone(), crate::props_of(&reply.child_value(0)));
                        }
                    }
                }
                raw.iwd = Some(objects);
            }
            Err(err) if err.matches(gio::DBusError::AccessDenied) => raw.iwd_denied = true,
            Err(_) => {}
        }
    }
    if has_owner(bus, NETWORKD) {
        if let Ok(reply) = call_to(bus, NETWORKD, NETWORKD_PATH, "org.freedesktop.network1.Manager", "Describe", None) {
            raw.networkd = reply.child_value(0).str().and_then(|json| serde_json::from_str(json).ok());
        }
    }
    if raw.iwd.is_some() || raw.networkd.is_some() {
        raw.addresses = interface_addresses();
        raw.default_routes = std::fs::read_to_string("/proc/net/route").map(|t| default_routes(&t)).unwrap_or_default();
    }
    build(&raw)
}

#[cfg(test)]
mod tests;

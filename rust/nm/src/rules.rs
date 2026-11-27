//! As regras do ícone e do popup de rede, sobre um [`Snapshot`]: o porte das funções de
//! `home/.local/bin/waybar-network` e `waybar-network-popup` (sem GTK, testadas com estados de mentira).
use std::collections::HashMap;

use swaydesk_common::i18n::tr;

use crate::{active_state, connectivity, AccessPoint, Active, Backend, Connection, Device, DeviceKind, Snapshot};

pub const VPN_TYPES: [&str; 2] = ["vpn", "wireguard"];
pub const WIFI_TYPE: &str = "802-11-wireless";

// NM80211ApFlags e NM80211ApSecurityFlags
const AP_PRIVACY: u32 = 0x1;
const SEC_PSK: u32 = 0x100;
const SEC_8021X: u32 = 0x200;
const SEC_SAE: u32 = 0x400;
const SEC_OWE: u32 = 0x800;
const SEC_OWE_TM: u32 = 0x1000;
const SEC_SUITE_B: u32 = 0x2000;

pub fn bars(strength: u8) -> u8 {
    match strength {
        0..25 => 1,
        25..50 => 2,
        50..75 => 3,
        _ => 4,
    }
}

/// (rótulo, é corporativa). A corporativa (802.1X) precisa do editor.
pub fn security_of(ap: &AccessPoint) -> (String, bool) {
    let both = ap.wpa | ap.rsn;
    if both & (SEC_8021X | SEC_SUITE_B) != 0 {
        return ("WPA Enterprise".into(), true);
    }
    if both & SEC_SAE != 0 {
        return ((if both & SEC_PSK != 0 { "WPA2/WPA3" } else { "WPA3" }).into(), false);
    }
    if both & SEC_PSK != 0 {
        return ((if ap.rsn & SEC_PSK != 0 { "WPA2" } else { "WPA" }).into(), false);
    }
    if both & (SEC_OWE | SEC_OWE_TM) != 0 {
        return ("OWE".into(), false);
    }
    if ap.flags & AP_PRIVACY != 0 {
        return ("WEP".into(), false);
    }
    (tr("Open"), false)
}

/// Com cadeado: tudo menos a aberta e a OWE.
pub fn locked(ap: &AccessPoint) -> bool {
    let label = security_of(ap).0;
    label != tr("Open") && label != "OWE"
}

pub fn freq_band(mhz: u32) -> String {
    match mhz {
        0..3000 => tr("2.4 GHz"),
        3000..5925 => "5 GHz".into(),
        _ => "6 GHz".into(),
    }
}

pub fn vpn_type(conn: &Connection) -> String {
    if conn.kind == "wireguard" {
        return "WireGuard".into();
    }
    let name = conn.vpn_service.rsplit('.').next().unwrap_or("");
    match name {
        "openvpn" => "OpenVPN",
        "openconnect" => "OpenConnect",
        "vpnc" => "Cisco (vpnc)",
        "l2tp" => "L2TP",
        "pptp" => "PPTP",
        "strongswan" => "strongSwan",
        "sstp" => "SSTP",
        "fortisslvpn" => "FortiSSL",
        "" => "VPN",
        other => return other.to_string(),
    }
    .into()
}

/// Os perfis Wi-Fi salvos que servem neste dispositivo, por SSID.
pub fn saved_by_ssid<'a>(snap: &'a Snapshot, device: &Device) -> HashMap<String, Vec<&'a Connection>> {
    let mut saved: HashMap<String, Vec<&Connection>> = HashMap::new();
    for conn in &snap.connections {
        let Some(ssid) = &conn.ssid else { continue };
        if !matches!(conn.mode.as_str(), "" | "infrastructure") {
            continue;
        }
        if !conn.iface.is_empty() && conn.iface != device.iface {
            continue;
        }
        saved.entry(ssid.clone()).or_default().push(conn);
    }
    saved
}

/// Uma rede da lista: o ponto de acesso de sinal mais forte de cada SSID.
#[derive(Debug, Clone, PartialEq)]
pub struct Network<'a> {
    pub ssid: String,
    pub ap: &'a AccessPoint,
    pub bars: u8,
    pub saved: Vec<&'a Connection>,
    pub active: bool,
}

/// Uma entrada por SSID; a ativa primeiro, depois as salvas, depois por força, e o nome desempata.
pub fn group_networks<'a>(snap: &'a Snapshot, device: &Device) -> Vec<Network<'a>> {
    let saved = saved_by_ssid(snap, device);
    let active = snap.access_point(device.active_access_point.as_deref());
    let active_ssid = active.map(|a| a.ssid.clone()).unwrap_or_default();
    let mut nets: Vec<(String, &AccessPoint)> = Vec::new(); // na ordem de chegada, como o dict do Python
    for ap in device.access_points.iter().filter_map(|p| snap.access_points.get(p)) {
        if ap.ssid.is_empty() {
            continue; // rede oculta: não dá para listar, tem botão próprio
        }
        match nets.iter_mut().find(|(s, _)| *s == ap.ssid) {
            Some(cur) if ap.strength > cur.1.strength => cur.1 = ap,
            Some(_) => {}
            None => nets.push((ap.ssid.clone(), ap)),
        }
    }
    let mut rows: Vec<Network> = nets
        .into_iter()
        .map(|(ssid, mut ap)| {
            if let Some(a) = active.filter(|_| ssid == active_ssid) {
                ap = a;
            }
            Network { bars: bars(ap.strength), saved: saved.get(&ssid).cloned().unwrap_or_default(), active: ssid == active_ssid, ssid, ap }
        })
        .collect();
    rows.sort_by(|a, b| {
        (!a.active, a.saved.is_empty(), std::cmp::Reverse(a.bars), casefold(&a.ssid))
            .cmp(&(!b.active, b.saved.is_empty(), std::cmp::Reverse(b.bars), casefold(&b.ssid)))
    });
    rows
}

/// O `str.casefold` do Python, bom o bastante para ordenar nomes (o `ß` vira `ss`).
pub fn casefold(text: &str) -> String {
    text.to_lowercase().replace('ß', "ss")
}

pub fn wifi_devices(snap: &Snapshot) -> Vec<&Device> {
    snap.devices.iter().filter(|d| d.kind == DeviceKind::Wifi).collect()
}

pub fn wired_devices(snap: &Snapshot) -> Vec<&Device> {
    snap.devices.iter().filter(|d| d.kind == DeviceKind::Ethernet && d.state != crate::device_state::UNMANAGED).collect()
}

/// As VPNs salvas, com a ativa de cada uma: as ligadas primeiro, depois pelo nome.
pub fn vpn_rows(snap: &Snapshot) -> Vec<(&Connection, Option<&Active>)> {
    let active: HashMap<&str, &Active> =
        snap.actives.iter().filter(|a| VPN_TYPES.contains(&a.kind.as_str())).map(|a| (a.uuid.as_str(), a)).collect();
    let mut rows: Vec<(&Connection, Option<&Active>)> =
        snap.connections.iter().filter(|c| VPN_TYPES.contains(&c.kind.as_str())).map(|c| (c, active.get(c.uuid.as_str()).copied())).collect();
    rows.sort_by_key(|(c, a)| (a.is_none(), casefold(&c.id)));
    rows
}

/// Nenhuma placa de Wi-Fi no PC (só a de cabo): o popup mostra o Wi-Fi desativado. Uma placa bloqueada
/// pelo rfkill continua contando (no NM ela segue na lista, e o interruptor a desbloqueia). Sem serviço no
/// ar, com o NM ainda criando as placas ou com o iwd recusando a leitura, não se sabe, e nada muda.
pub fn wifi_absent(snap: &Snapshot) -> bool {
    snap.running && !snap.starting && !snap.iwd_denied && !snap.wifi_adapter && wifi_devices(snap).is_empty()
}

/// O motivo de não haver busca de redes, ou None.
pub fn wifi_blocked(snap: &Snapshot) -> Option<String> {
    if !snap.networking {
        return Some(tr("Networking disabled"));
    }
    if !snap.wireless_hardware {
        return Some(tr("Wi-Fi blocked by the hardware switch"));
    }
    if !snap.wireless {
        return Some(tr("Wi-Fi off"));
    }
    None
}

// --- o ícone ---

/// Material da Nerd Font: wifi-strength-1..4, com alerta (conectividade limitada)
pub fn wifi_icon(bars: u8, alert: bool) -> &'static str {
    match (bars, alert) {
        (1, false) => "\u{f091f}",
        (2, false) => "\u{f0922}",
        (3, false) => "\u{f0925}",
        (_, false) => "\u{f0928}",
        (1, true) => "\u{f0920}",
        (2, true) => "\u{f0923}",
        (3, true) => "\u{f0926}",
        (_, true) => "\u{f0929}",
    }
}
pub const WIFI_OFF: &str = "\u{f092d}";
/// Ligado, sem conexão
pub const WIFI_NONE: &str = "\u{f092f}";
pub const ETHERNET: &str = "\u{f0200}";
pub const VPN: &str = "\u{f0582}";
/// lan-disconnect
pub const OFF: &str = "\u{f0318}";

fn first_ip(snap: &Snapshot, active: &Active) -> String {
    active
        .ip4
        .as_deref()
        .and_then(|p| snap.ip.get(p))
        .and_then(|c| c.addresses.first())
        .map(|a| a.split('/').next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// O aviso de que não há com quem falar: sem nenhum serviço, ou com o iwd recusando a leitura.
pub fn not_running(snap: &Snapshot) -> Option<String> {
    if !snap.running {
        return Some(tr("No network service running"));
    }
    None
}

/// A linha extra da dica com o iwd recusando (o usuário fora dos grupos da política do D-Bus dele).
fn denied_note(snap: &Snapshot) -> Vec<String> {
    if snap.iwd_denied { vec![tr("No permission to use iwd")] } else { vec![] }
}

/// (ícone, classe, linhas da dica, sem escape), como o `state` do waybar-network.
pub fn icon_state(snap: &Snapshot) -> (&'static str, &'static str, Vec<String>) {
    if let Some(text) = not_running(snap) {
        return (OFF, "disconnected", vec![text]);
    }
    if !snap.networking {
        return (OFF, "disconnected", vec![tr("Networking disabled")]);
    }
    let vpns: Vec<&str> = snap
        .actives
        .iter()
        .filter(|a| VPN_TYPES.contains(&a.kind.as_str()) && a.state == active_state::ACTIVATED)
        .map(|a| a.id.as_str())
        .collect();
    let Some(primary) = snap.active(snap.primary.as_deref()) else {
        if let Some(pending) = snap.active(snap.activating.as_deref()) {
            return (WIFI_NONE, "connecting", vec![tr("Connecting to %s").replace("%s", &pending.id)]);
        }
        // Só o networkd (ou o iwd sem placa): não há Wi-Fi para dizer que está desligado
        if snap.backend == Backend::Standalone && wifi_devices(snap).is_empty() {
            return (OFF, "disconnected", [vec![tr("Not connected")], denied_note(snap)].concat());
        }
        if !snap.wireless {
            return (WIFI_OFF, "disconnected", vec![tr("Wi-Fi off")]);
        }
        return (WIFI_NONE, "disconnected", vec![tr("Not connected")]);
    };
    let limited = matches!(snap.connectivity, connectivity::LIMITED | connectivity::PORTAL | connectivity::NONE);
    let note: Vec<String> = if limited { vec![tr("No internet access")] } else { vec![] };
    let extra: Vec<String> = vpns.iter().filter(|v| **v != primary.id).map(|v| format!("VPN: {v}")).collect();
    let ip = first_ip(snap, primary);
    if VPN_TYPES.contains(&primary.kind.as_str()) {
        return (VPN, "vpn", [vec![format!("VPN: {}", primary.id), ip], note].concat());
    }
    if primary.kind == WIFI_TYPE {
        let ap = primary.devices.first().and_then(|d| snap.device(d)).and_then(|d| snap.access_point(d.active_access_point.as_deref()));
        let strength = ap.map(|a| a.strength).unwrap_or(0);
        let title = ap.map(|a| a.ssid.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| primary.id.clone());
        let lines = [vec![format!("{title} · {strength}%"), ip], note, extra].concat();
        return (wifi_icon(bars(strength), limited), if limited { "limited" } else { "wifi" }, lines);
    }
    (ETHERNET, if limited { "limited" } else { "ethernet" }, [vec![primary.id.clone(), ip], note, extra, denied_note(snap)].concat())
}

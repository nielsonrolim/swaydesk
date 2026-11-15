use super::*;
use crate::{group_networks, icon_state, wifi_devices, wired_devices};
use gio::glib::prelude::*;

fn props(pairs: &[(&str, Variant)]) -> Props {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn path(p: &str) -> Variant {
    gio::glib::variant::ObjectPath::try_from(p.to_string()).unwrap().to_variant()
}

const DEV: &str = "/net/connman/iwd/0/4";
const HOME: &str = "/net/connman/iwd/0/4/63617361_psk";
const OPEN: &str = "/net/connman/iwd/0/4/636166e9_open";
const CORP: &str = "/net/connman/iwd/0/4/636f7270_8021x";

/// Uma placa conectada à "casa" (salva), com um café aberto e uma rede corporativa por perto.
fn iwd(state: &str, connected: Option<&str>) -> Objects {
    let mut objects = Objects::new();
    objects.insert("/net/connman/iwd/0".into(), HashMap::from([(IWD_ADAPTER.to_string(), props(&[("Powered", true.to_variant())]))]));
    let mut station = vec![("State", state.to_variant()), ("Scanning", false.to_variant())];
    if let Some(c) = connected {
        station.push(("ConnectedNetwork", path(c)));
    }
    objects.insert(DEV.into(), HashMap::from([
        (IWD_DEVICE.to_string(), props(&[("Name", "wlan0".to_variant()), ("Address", "02:00:00:00:01:00".to_variant()), ("Powered", true.to_variant()), ("Adapter", path("/net/connman/iwd/0")), ("Mode", "station".to_variant())])),
        (IWD_STATION.to_string(), props(&station)),
        (IWD_DIAGNOSTIC.to_string(), Props::new()),
    ]));
    for (p, name, kind) in [(HOME, "casa", "psk"), (OPEN, "café <livre>", "open"), (CORP, "corp", "8021x")] {
        objects.insert(p.into(), HashMap::from([(IWD_NETWORK.to_string(), props(&[("Name", name.to_variant()), ("Type", kind.to_variant()), ("Connected", (Some(p) == connected).to_variant()), ("Device", path(DEV))]))]));
    }
    objects.insert("/net/connman/iwd/63617361_psk".into(), HashMap::from([(IWD_KNOWN.to_string(), props(&[("Name", "casa".to_variant()), ("Type", "psk".to_variant())]))]));
    objects
}

fn raw(objects: Objects) -> Raw {
    Raw {
        iwd: Some(objects),
        ordered: HashMap::from([(DEV.to_string(), vec![(HOME.to_string(), -4500), (OPEN.to_string(), -7000), (CORP.to_string(), -9000)])]),
        diagnostics: HashMap::from([(DEV.to_string(), props(&[("Frequency", 5180u32.to_variant()), ("TxBitrate", 866_000u32.to_variant()), ("RSSI", (-45i16).to_variant())]))]),
        addresses: HashMap::from([("wlan0".to_string(), (vec!["192.168.1.20/24".to_string()], vec!["fe80::1/64".to_string()]))]),
        ..Default::default()
    }
}

fn networkd(oper: &str, admin: &str) -> Value {
    serde_json::json!({"Interfaces": [
        {"Name": "lo", "Type": "loopback", "AdministrativeState": "unmanaged", "OperationalState": "carrier"},
        {"Name": "enp1s0", "Type": "ether", "AdministrativeState": admin, "OperationalState": oper,
         "HardwareAddress": [82, 84, 0, 18, 52, 86],
         "Routes": [{"Family": 2, "Destination": [0, 0, 0, 0], "DestinationPrefixLength": 0, "Gateway": [10, 0, 2, 2]},
                    {"Family": 2, "Destination": [10, 0, 2, 0], "DestinationPrefixLength": 24}],
         "DNS": [{"Family": 2, "Address": [10, 0, 2, 3]}, {"Family": 10, "Address": [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]}]},
        {"Name": "docker0", "Type": "ether", "AdministrativeState": "unmanaged", "OperationalState": "routable"},
        {"Name": "wlan0", "Type": "wlan", "AdministrativeState": "unmanaged", "OperationalState": "routable",
         "Routes": [{"Family": 2, "DestinationPrefixLength": 0, "Gateway": [192, 168, 1, 1]}]}
    ]})
}

#[test]
fn signal_in_percent() {
    assert_eq!([-3000, -5000, -6000, -7500, -10000, -11000].map(strength_of), [100, 100, 80, 50, 0, 0]);
}

#[test]
fn connected_wifi_maps_to_the_nm_shapes() {
    let snap = build(&raw(iwd("connected", Some(HOME))));
    assert_eq!(snap.backend, Backend::Standalone);
    assert!(snap.running && snap.networking && snap.wireless && snap.wireless_hardware);
    let devs = wifi_devices(&snap);
    assert_eq!(devs.len(), 1);
    let dev = devs[0];
    assert_eq!((dev.iface.as_str(), dev.state, dev.bitrate), ("wlan0", device_state::ACTIVATED, 866_000));
    assert_eq!(dev.active_access_point.as_deref(), Some(HOME));
    let ap = &snap.access_points[HOME];
    assert_eq!((ap.ssid.as_str(), ap.strength, ap.frequency), ("casa", 100, 5180));
    assert_eq!(crate::security_of(ap).0, "WPA2");
    assert!(crate::security_of(&snap.access_points[CORP]).1, "a 802.1X é corporativa");
    assert!(!crate::locked(&snap.access_points[OPEN]));
    // A salva (KnownNetwork) aparece como salva, e a ativa vem primeiro
    let rows = group_networks(&snap, dev);
    assert_eq!(rows.iter().map(|r| r.ssid.as_str()).collect::<Vec<_>>(), ["casa", "café <livre>", "corp"]);
    assert!(rows[0].active && !rows[0].saved.is_empty() && rows[1].saved.is_empty());
    assert_eq!(snap.primary.as_deref(), Some(HOME));
    let active = snap.active(Some(HOME)).unwrap();
    assert_eq!(snap.ip[active.ip4.as_deref().unwrap()].addresses, ["192.168.1.20/24"]);
    let (icon, class, lines) = icon_state(&snap);
    assert_eq!((icon, class), (crate::wifi_icon(4, false), "wifi"));
    assert_eq!(lines, ["casa · 100%", "192.168.1.20"]);
}

#[test]
fn connecting_and_disconnected() {
    let snap = build(&raw(iwd("connecting", Some(OPEN))));
    assert_eq!(snap.primary, None);
    assert_eq!(snap.activating.as_deref(), Some(OPEN));
    assert!(device_state::CONNECTING.contains(&snap.devices[0].state));
    assert_eq!(icon_state(&snap).1, "connecting");

    let snap = build(&raw(iwd("disconnected", None)));
    assert_eq!((snap.primary.clone(), snap.activating.clone()), (None, None));
    assert!(snap.actives.is_empty());
    assert_eq!(snap.devices[0].state, device_state::DISCONNECTED);
    assert_eq!(icon_state(&snap).1, "disconnected");
}

#[test]
fn powered_off_card_has_no_station() {
    let mut objects = iwd("disconnected", None);
    let dev = objects.get_mut(DEV).unwrap();
    dev.remove(IWD_STATION);
    dev.get_mut(IWD_DEVICE).unwrap().insert("Powered".into(), false.to_variant());
    let mut r = raw(objects);
    r.ordered.clear();
    let snap = build(&r);
    assert!(!snap.wireless);
    assert_eq!(snap.devices[0].state, device_state::UNAVAILABLE);
    assert_eq!(crate::wifi_blocked(&snap), Some(swaydesk_common::i18n::tr("Wi-Fi off")));
    assert_eq!(icon_state(&snap).0, crate::WIFI_OFF);
}

#[test]
fn an_access_point_device_is_not_listed() {
    let mut objects = iwd("disconnected", None);
    objects.get_mut(DEV).unwrap().get_mut(IWD_DEVICE).unwrap().insert("Mode".into(), "ap".to_variant());
    assert!(wifi_devices(&build(&raw(objects))).is_empty());
}

#[test]
fn wired_from_networkd() {
    let r = Raw {
        networkd: Some(networkd("routable", "configured")),
        addresses: HashMap::from([("enp1s0".to_string(), (vec!["10.0.2.15/24".to_string()], vec![]))]),
        ..Default::default()
    };
    let snap = build(&r);
    let wired = wired_devices(&snap);
    assert_eq!(wired.iter().map(|d| d.iface.as_str()).collect::<Vec<_>>(), ["enp1s0"], "o docker0 e o lo, fora do networkd, não entram");
    assert_eq!(wired[0].hw_address, "52:54:00:12:34:56");
    assert_eq!(wired[0].state, device_state::ACTIVATED);
    let primary = snap.active(snap.primary.as_deref()).unwrap();
    let ip4 = &snap.ip[primary.ip4.as_deref().unwrap()];
    assert_eq!((ip4.gateway.as_str(), ip4.dns.clone()), ("10.0.2.2", vec!["10.0.2.3".to_string()]));
    assert_eq!(icon_state(&snap), (crate::ETHERNET, "ethernet", vec!["enp1s0".to_string(), "10.0.2.15".to_string()]));

    for (oper, admin, state) in [("no-carrier", "configured", device_state::UNAVAILABLE), ("degraded", "configuring", device_state::CONNECTING[0]), ("off", "failed", device_state::UNAVAILABLE), ("carrier", "failed", device_state::DISCONNECTED)] {
        let snap = build(&Raw { networkd: Some(networkd(oper, admin)), ..Default::default() });
        assert_eq!(snap.devices[0].state, state, "{oper}/{admin}");
        assert_eq!(snap.primary, None);
    }
    // Só o networkd, com o cabo fora: o ícone não fala de um Wi-Fi que não existe
    let snap = build(&Raw { networkd: Some(networkd("no-carrier", "configured")), ..Default::default() });
    assert_eq!(icon_state(&snap), (crate::OFF, "disconnected", vec![swaydesk_common::i18n::tr("Not connected")]));
}

#[test]
fn cable_wins_over_wifi_and_wifi_gets_the_networkd_gateway() {
    let mut r = raw(iwd("connected", Some(HOME)));
    r.networkd = Some(networkd("routable", "configured"));
    let snap = build(&r);
    assert_eq!(snap.primary.as_deref(), Some("networkd:enp1s0"));
    let wifi = snap.active(Some(HOME)).unwrap();
    assert_eq!(snap.ip[wifi.ip4.as_deref().unwrap()].gateway, "192.168.1.1");
}

#[test]
fn the_route_with_the_lowest_metric_is_the_primary() {
    let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                 enp1s0\t00000000\t0202000A\t0003\t0\t0\t1024\t00000000\t0\t0\t0\n\
                 wlan0\t00000000\t0101A8C0\t0003\t0\t0\t305\t00000000\t0\t0\t0\n\
                 wlan0\t0001A8C0\t00000000\t0001\t0\t0\t305\t00FFFFFF\t0\t0\t0\n";
    assert_eq!(default_routes(table), [("enp1s0".to_string(), 1024), ("wlan0".to_string(), 305)]);
    let mut r = raw(iwd("connected", Some(HOME)));
    r.networkd = Some(networkd("routable", "configured"));
    r.default_routes = default_routes(table);
    let snap = build(&r);
    assert_eq!(snap.primary.as_deref(), Some(HOME), "o Wi-Fi, de métrica menor, leva o tráfego");
    assert_eq!(icon_state(&snap).1, "wifi");
    // A rota do cabo com métrica menor volta a ele
    r.default_routes = vec![("enp1s0".into(), 100), ("wlan0".into(), 600)];
    assert_eq!(build(&r).primary.as_deref(), Some("networkd:enp1s0"));
}

#[test]
fn nothing_running_and_denied() {
    let snap = build(&Raw::default());
    assert_eq!(snap, Snapshot::default());
    assert_eq!(snap.backend, Backend::None);
    assert_eq!(icon_state(&snap).2, [swaydesk_common::i18n::tr("No network service running")]);

    let snap = build(&Raw { iwd_denied: true, ..Default::default() });
    assert!(snap.running && snap.iwd_denied);
    assert_eq!(icon_state(&snap).2, [swaydesk_common::i18n::tr("Not connected"), swaydesk_common::i18n::tr("No permission to use iwd")]);
}

#[test]
fn networkd_addresses_and_macs() {
    assert_eq!(ip_text(&serde_json::json!([10, 0, 0, 1])).as_deref(), Some("10.0.0.1"));
    assert_eq!(ip_text(&serde_json::json!([1, 2, 3])), None);
    assert_eq!(ip_text(&serde_json::json!([300, 0, 0, 1])), None);
    assert_eq!(mac_text(&serde_json::json!("aa:bb:cc:dd:ee:ff")), "aa:bb:cc:dd:ee:ff");
}

#[test]
fn ordered_networks_from_dbus() {
    let reply = Variant::tuple_from_iter([Variant::array_from_iter_with_type(
        gio::glib::VariantTy::new("(on)").unwrap(),
        [Variant::tuple_from_iter([path(HOME), (-4500i16).to_variant()])],
    )]);
    assert_eq!(ordered_networks(&reply), [(HOME.to_string(), -4500)]);
}

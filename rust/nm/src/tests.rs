use super::*;

fn ap(path: &str, ssid: &str, strength: u8) -> AccessPoint {
    AccessPoint { path: path.into(), ssid: ssid.into(), strength, ..Default::default() }
}

fn wifi_device(aps: &[&str], active: Option<&str>) -> Device {
    Device {
        path: "/d/1".into(),
        iface: "wlan0".into(),
        kind: DeviceKind::Wifi,
        state: device_state::ACTIVATED,
        hw_address: String::new(),
        active_connection: None,
        access_points: aps.iter().map(|s| s.to_string()).collect(),
        active_access_point: active.map(String::from),
        bitrate: 0,
        speed: 0,
    }
}

fn snapshot(aps: Vec<AccessPoint>, device: Device) -> Snapshot {
    Snapshot {
        running: true,
        networking: true,
        wireless: true,
        wireless_hardware: true,
        access_points: aps.into_iter().map(|a| (a.path.clone(), a)).collect(),
        devices: vec![device],
        ..Default::default()
    }
}

#[test]
fn bars_and_bands() {
    assert_eq!([0, 24, 25, 49, 50, 74, 75, 100].map(bars), [1, 1, 2, 2, 3, 3, 4, 4]);
    assert_eq!(freq_band(5180), "5 GHz");
    assert_eq!(freq_band(5955), "6 GHz");
}

#[test]
fn security_labels() {
    let with = |flags, wpa, rsn| security_of(&AccessPoint { flags, wpa, rsn, ..Default::default() });
    assert_eq!(with(1, 0, 0x200), ("WPA Enterprise".into(), true));
    assert_eq!(with(1, 0, 0x400 | 0x100), ("WPA2/WPA3".into(), false));
    assert_eq!(with(1, 0, 0x400), ("WPA3".into(), false));
    assert_eq!(with(1, 0, 0x100), ("WPA2".into(), false));
    assert_eq!(with(1, 0x100, 0), ("WPA".into(), false));
    assert_eq!(with(0, 0, 0x800), ("OWE".into(), false));
    assert_eq!(with(1, 0, 0), ("WEP".into(), false));
    assert!(!with(0, 0, 0).1);
    assert!(!locked(&AccessPoint::default()));
    assert!(locked(&AccessPoint { rsn: 0x100, ..Default::default() }));
}

#[test]
fn networks_grouped_by_ssid_and_sorted() {
    let aps = vec![ap("/a/1", "Casa", 40), ap("/a/2", "Casa", 80), ap("/a/3", "", 99), ap("/a/4", "bar", 90), ap("/a/5", "Alfa", 90), ap("/a/6", "Vizinho", 30)];
    let device = wifi_device(&["/a/1", "/a/2", "/a/3", "/a/4", "/a/5", "/a/6"], Some("/a/6"));
    let mut snap = snapshot(aps, device.clone());
    snap.connections.push(Connection { ssid: Some("Casa".into()), kind: "802-11-wireless".into(), ..Default::default() });
    // Um perfil de outra placa e um de ponto de acesso não servem
    snap.connections.push(Connection { ssid: Some("bar".into()), iface: "wlan1".into(), ..Default::default() });
    snap.connections.push(Connection { ssid: Some("Alfa".into()), mode: "ap".into(), ..Default::default() });
    let rows = group_networks(&snap, &device);
    let names: Vec<(&str, u8, bool, bool)> = rows.iter().map(|r| (r.ssid.as_str(), r.bars, !r.saved.is_empty(), r.active)).collect();
    assert_eq!(names, [("Vizinho", 2, false, true), ("Casa", 4, true, false), ("Alfa", 4, false, false), ("bar", 4, false, false)]);
    assert_eq!(rows[1].ap.path, "/a/2"); // o mais forte da rede
}

#[test]
fn vpns_active_first_then_by_name() {
    let mut snap = Snapshot::default();
    for (id, uuid, kind) in [("Zeta", "z", "vpn"), ("alfa", "a", "wireguard"), ("Casa", "c", "802-11-wireless"), ("Beta", "b", "vpn")] {
        snap.connections.push(Connection { id: id.into(), uuid: uuid.into(), kind: kind.into(), vpn_service: "org.freedesktop.NetworkManager.openvpn".into(), ..Default::default() });
    }
    snap.actives.push(Active { uuid: "z".into(), kind: "vpn".into(), ..Default::default() });
    let rows: Vec<(&str, bool)> = vpn_rows(&snap).iter().map(|(c, a)| (c.id.as_str(), a.is_some())).collect();
    assert_eq!(rows, [("Zeta", true), ("alfa", false), ("Beta", false)]);
    assert_eq!(vpn_type(&snap.connections[0]), "OpenVPN");
    assert_eq!(vpn_type(&snap.connections[1]), "WireGuard");
    assert_eq!(vpn_type(&Connection { vpn_service: "org.x.novo".into(), ..Default::default() }), "novo");
    assert_eq!(vpn_type(&Connection::default()), "VPN");
}

#[test]
fn icon_states() {
    let device = wifi_device(&["/a/1"], Some("/a/1"));
    let mut snap = snapshot(vec![ap("/a/1", "Casa <&>", 60)], device);
    snap.actives.push(Active {
        path: "/ac/1".into(), id: "Casa".into(), kind: WIFI_TYPE.into(), state: 2, devices: vec!["/d/1".into()], ip4: Some("/ip/4".into()), ..Default::default()
    });
    snap.ip.insert("/ip/4".into(), IpConfig { addresses: vec!["192.168.0.10/24".into()], ..Default::default() });
    snap.primary = Some("/ac/1".into());
    snap.connectivity = 4;
    let (icon, class, lines) = icon_state(&snap);
    assert_eq!((icon, class), (wifi_icon(3, false), "wifi"));
    assert_eq!(lines, ["Casa <&> · 60%", "192.168.0.10"]);
    snap.connectivity = connectivity::PORTAL;
    let (icon, class, lines) = icon_state(&snap);
    assert_eq!((icon, class, lines.len()), (wifi_icon(3, true), "limited", 3));
    snap.primary = None;
    snap.activating = Some("/ac/1".into());
    assert_eq!(icon_state(&snap).1, "connecting");
    snap.activating = None;
    snap.wireless = false;
    assert_eq!(icon_state(&snap).0, WIFI_OFF);
    snap.networking = false;
    assert_eq!(icon_state(&snap).0, OFF);
    assert_eq!(icon_state(&Snapshot::default()).1, "disconnected");
}

#[test]
fn ssid_bytes() {
    assert_eq!(ssid_to_utf8("Café".as_bytes()), "Café");
    assert_eq!(ssid_to_utf8(&[0x43, 0x61, 0x66, 0xe9]), "Café"); // Latin-1
}

#[test]
fn wifi_blocked_reasons() {
    let mut snap = snapshot(vec![], wifi_device(&[], None));
    assert_eq!(wifi_blocked(&snap), None);
    snap.wireless = false;
    assert!(wifi_blocked(&snap).is_some());
}

//! O estado do BlueZ pelo D-Bus, sem GTK: o que o `waybar-bluetooth` e o `waybar-bluetooth-popup` em Python
//! liam do `GetManagedObjects`. Nada aqui lida com PIN ou confirmação de pareamento: quem responde é o
//! agente do blueman (blueman-agent).
use std::collections::BTreeMap;

use gio::glib::{self, Variant};
use swaydesk_common::i18n::tr;

pub mod rfkill;

pub const BLUEZ: &str = "org.bluez";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub powered: bool,
    pub discoverable: bool,
    pub discovering: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub alias: String,
    pub address: String,
    /// O Name do BlueZ: sem ele, o aparelho só mostraria o endereço
    pub name: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub battery: Option<u8>,
}

impl Device {
    /// O nome que aparece: o apelido ou, sem ele, o endereço.
    pub fn title(&self) -> &str {
        if self.alias.is_empty() { &self.address } else { &self.alias }
    }
}

/// Uma leitura. `adapter` é o primeiro em ordem de caminho; `devices`, só os dele, em ordem de caminho.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
}

fn props_of(dict: &Variant) -> BTreeMap<String, Variant> {
    (0..dict.n_children())
        .filter_map(|i| {
            let e = dict.child_value(i);
            Some((e.child_value(0).str()?.to_string(), e.child_value(1).as_variant()?))
        })
        .collect()
}

fn flag(props: &BTreeMap<String, Variant>, key: &str) -> bool {
    props.get(key).and_then(|v| v.get::<bool>()).unwrap_or(false)
}

fn text(props: &BTreeMap<String, Variant>, key: &str) -> String {
    props.get(key).and_then(|v| v.str().map(String::from)).unwrap_or_default()
}

/// Lê o BlueZ. None: o bluetoothd não está no ar. Síncrono: fora da linha da interface.
pub fn read(bus: &gio::DBusConnection) -> Option<State> {
    let reply = bus
        .call_sync(Some(BLUEZ), "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", None, None, gio::DBusCallFlags::NONE, 3000, gio::Cancellable::NONE)
        .ok()?;
    let objects = reply.child_value(0);
    let mut by_path = Objects::new();
    for i in 0..objects.n_children() {
        let entry = objects.child_value(i);
        let Some(path) = entry.child_value(0).str().map(String::from) else { continue };
        let ifaces = entry.child_value(1);
        let found = (0..ifaces.n_children())
            .filter_map(|j| {
                let iface = ifaces.child_value(j);
                Some((iface.child_value(0).str()?.to_string(), props_of(&iface.child_value(1))))
            })
            .collect();
        by_path.insert(path, found);
    }
    Some(state_of(&by_path))
}

/// Caminho → interface → propriedades, em ordem de caminho.
pub type Objects = BTreeMap<String, BTreeMap<String, BTreeMap<String, Variant>>>;

/// O estado a partir dos objetos.
pub fn state_of(objects: &Objects) -> State {
    let mut state = State::default();
    let Some((path, ifaces)) = objects.iter().find(|(_, i)| i.contains_key(ADAPTER)) else { return state };
    let a = &ifaces[ADAPTER];
    state.adapter = Some(Adapter { path: path.clone(), powered: flag(a, "Powered"), discoverable: flag(a, "Discoverable"), discovering: flag(a, "Discovering") });
    let prefix = format!("{path}/");
    for (path, ifaces) in objects.iter().filter(|(p, _)| p.starts_with(&prefix)) {
        let Some(d) = ifaces.get(DEVICE) else { continue };
        state.devices.push(Device {
            path: path.clone(),
            alias: text(d, "Alias"),
            address: text(d, "Address"),
            name: text(d, "Name"),
            icon: text(d, "Icon"),
            paired: flag(d, "Paired"),
            connected: flag(d, "Connected"),
            trusted: flag(d, "Trusted"),
            battery: ifaces.get(BATTERY).and_then(|b| b.get("Percentage")).and_then(|v| v.get::<u8>()),
        });
    }
    state
}

/// O `str.casefold` do Python, bom o bastante para ordenar nomes.
fn casefold(text: &str) -> String {
    text.to_lowercase().replace('ß', "ss")
}

/// (meus: pareados ou conectados, os conectados primeiro; outros: com nome, por nome). Sem nome o aparelho
/// só mostraria o endereço, e a ordem por nome não pula sob o cursor.
pub fn split_devices(devices: &[Device]) -> (Vec<&Device>, Vec<&Device>) {
    let mut mine: Vec<&Device> = devices.iter().filter(|d| d.paired || d.connected).collect();
    mine.sort_by_key(|d| (!d.connected, casefold(&d.alias)));
    let mut others: Vec<&Device> = devices.iter().filter(|d| !(d.paired || d.connected || d.name.is_empty())).collect();
    others.sort_by_key(|d| casefold(&d.alias));
    (mine, others)
}

// --- o ícone ---

/// Material da Nerd Font: bluetooth, bluetooth-connect, bluetooth-off
pub const ON: &str = "\u{f00af}";
pub const CONNECTED: &str = "\u{f00b1}";
pub const OFF: &str = "\u{f00b2}";

/// (ícone, classe, linhas da dica, sem escape), como o `state` do waybar-bluetooth. `state` None: o
/// bluetoothd parado.
pub fn icon_state(state: Option<&State>) -> (&'static str, &'static str, Vec<String>) {
    let Some(state) = state else { return (OFF, "off", vec![tr("Bluetooth service is not running")]) };
    let Some(adapter) = &state.adapter else { return (OFF, "off", vec![tr("No Bluetooth adapter")]) };
    if !adapter.powered {
        return (OFF, "off", vec![tr("Bluetooth off")]);
    }
    let lines: Vec<String> = state
        .devices
        .iter()
        .filter(|d| d.connected)
        .map(|d| match d.battery {
            Some(b) => format!("{} · {b}%", d.title()),
            None => d.title().to_string(),
        })
        .collect();
    if lines.is_empty() {
        return (ON, "on", vec![tr("No device connected")]);
    }
    (CONNECTED, "connected", lines)
}

#[allow(deprecated)] // o signal_subscribe com closure continua o caminho simples no gio 0.20
/// Assina tudo o que o BlueZ avisa e a troca de dono do nome. Devolve as assinaturas.
pub fn watch(bus: &gio::DBusConnection, changed: impl Fn() + Clone + 'static) -> Vec<gio::SignalSubscriptionId> {
    let on = changed.clone();
    vec![
        bus.signal_subscribe(Some(BLUEZ), None, None, None, None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| on()),
        bus.signal_subscribe(Some("org.freedesktop.DBus"), Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), Some("/org/freedesktop/DBus"), Some(BLUEZ), gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| changed()),
    ]
}

pub fn system_bus() -> Result<gio::DBusConnection, glib::Error> {
    gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gio::glib::variant::ToVariant;

    type Entry<'a> = (&'a str, &'a str, Vec<(&'a str, Variant)>);

    fn obj(entries: &[Entry]) -> Objects {
        let mut out = Objects::new();
        for (path, iface, props) in entries {
            out.entry(path.to_string()).or_default().insert(iface.to_string(), props.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
        }
        out
    }

    fn sample() -> State {
        state_of(&obj(&[
            ("/org/bluez/hci1", ADAPTER, vec![("Powered", false.to_variant())]),
            ("/org/bluez/hci0", ADAPTER, vec![("Powered", true.to_variant()), ("Discovering", true.to_variant())]),
            ("/org/bluez/hci0/dev_A", DEVICE, vec![("Alias", "zeta fone".to_variant()), ("Paired", true.to_variant()), ("Name", "z".to_variant())]),
            ("/org/bluez/hci0/dev_B", DEVICE, vec![("Alias", "Mouse <b>".to_variant()), ("Connected", true.to_variant()), ("Paired", true.to_variant())]),
            ("/org/bluez/hci0/dev_B", BATTERY, vec![("Percentage", 80u8.to_variant())]),
            ("/org/bluez/hci0/dev_C", DEVICE, vec![("Alias", "Alfa TV".to_variant()), ("Name", "Alfa TV".to_variant())]),
            ("/org/bluez/hci0/dev_D", DEVICE, vec![("Alias", "11-22".to_variant()), ("Address", "11:22".to_variant())]),
            ("/org/bluez/hci1/dev_E", DEVICE, vec![("Alias", "de outro".to_variant()), ("Connected", true.to_variant())]),
        ]))
    }

    #[test]
    fn first_adapter_and_its_devices() {
        let state = sample();
        let adapter = state.adapter.clone().unwrap();
        assert_eq!((adapter.path.as_str(), adapter.powered, adapter.discovering), ("/org/bluez/hci0", true, true));
        assert_eq!(state.devices.len(), 4);
        let (mine, others) = split_devices(&state.devices);
        assert_eq!(mine.iter().map(|d| d.alias.as_str()).collect::<Vec<_>>(), ["Mouse <b>", "zeta fone"]);
        assert_eq!(others.iter().map(|d| d.alias.as_str()).collect::<Vec<_>>(), ["Alfa TV"]); // sem Name fica de fora
    }

    #[test]
    fn icon_states() {
        let state = sample();
        assert_eq!(icon_state(Some(&state)), (CONNECTED, "connected", vec!["Mouse <b> · 80%".to_string()]));
        assert_eq!(icon_state(None).1, "off");
        assert_eq!(icon_state(Some(&State::default())).0, OFF);
        let mut off = state.clone();
        off.adapter.as_mut().unwrap().powered = false;
        assert_eq!(icon_state(Some(&off)).2.len(), 1);
        let mut none = state;
        none.devices.iter_mut().for_each(|d| d.connected = false);
        assert_eq!(icon_state(Some(&none)).1, "on");
    }

    #[test]
    fn title_falls_back_to_the_address() {
        assert_eq!(Device { address: "AA:BB".into(), ..Default::default() }.title(), "AA:BB");
    }
}

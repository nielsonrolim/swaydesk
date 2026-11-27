"""O NetworkManager falso do headless-network-popup.sh: o GetManagedObjects de um PC com uma placa de cabo
conectada e, com "wifi" (ou depois de um SIGUSR1, como uma placa USB espetada), uma placa de Wi-Fi com duas
redes por perto. Uso: fake-nm.py [wifi] (num barramento que o DBUS_SYSTEM_BUS_ADDRESS aponta). Nada de
perfis, segredos nem ativação: o popup só lê."""
import signal
import sys

from gi.repository import Gio, GLib

XML = """<node>
<interface name="org.freedesktop.DBus.ObjectManager">
  <method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method>
  <signal name="InterfacesAdded"><arg type="o"/><arg type="a{sa{sv}}"/></signal>
</interface>
</node>"""
NM = "org.freedesktop.NetworkManager"
BASE = "/org/freedesktop/NetworkManager"
WIRED, WIFI, ACTIVE = f"{BASE}/Devices/1", f"{BASE}/Devices/2", f"{BASE}/ActiveConnection/1"
APS = [(f"{BASE}/AccessPoint/1", "Casa", 82), (f"{BASE}/AccessPoint/2", "Café", 45)]
state = {"wifi": "wifi" in sys.argv[1:]}


def v(sig, value):
    return GLib.Variant(sig, value)


def objects():
    devices = [WIRED] + ([WIFI] if state["wifi"] else [])
    found = {
        BASE: {NM: {
            "NetworkingEnabled": v("b", True), "WirelessEnabled": v("b", True),
            "WirelessHardwareEnabled": v("b", True), "Connectivity": v("u", 4), "Startup": v("b", False),
            "PrimaryConnection": v("o", ACTIVE), "ActivatingConnection": v("o", "/"),
            "Devices": v("ao", devices), "ActiveConnections": v("ao", [ACTIVE]),
        }},
        WIRED: {
            f"{NM}.Device": {"Interface": v("s", "enp1s0"), "DeviceType": v("u", 1), "State": v("u", 100),
                             "ActiveConnection": v("o", ACTIVE), "HwAddress": v("s", "52:54:00:12:34:56")},
            f"{NM}.Device.Wired": {"HwAddress": v("s", "52:54:00:12:34:56"), "Speed": v("u", 1000)},
        },
        ACTIVE: {f"{NM}.Connection.Active": {
            "Id": v("s", "Wired connection 1"), "Uuid": v("s", "4f1c0a8e-0000-4000-8000-000000000001"),
            "Type": v("s", "802-3-ethernet"), "State": v("u", 2), "Devices": v("ao", [WIRED]),
            "Ip4Config": v("o", "/"), "Ip6Config": v("o", "/"),
        }},
        f"{BASE}/Settings": {f"{NM}.Settings": {"Connections": v("ao", [])}},
    }
    if state["wifi"]:
        found[WIFI] = {
            f"{NM}.Device": {"Interface": v("s", "wlan0"), "DeviceType": v("u", 2), "State": v("u", 30),
                             "ActiveConnection": v("o", "/"), "HwAddress": v("s", "02:00:00:00:01:00")},
            f"{NM}.Device.Wireless": {"HwAddress": v("s", "02:00:00:00:01:00"), "Bitrate": v("u", 0),
                                      "AccessPoints": v("ao", [p for p, _, _ in APS]), "ActiveAccessPoint": v("o", "/")},
        }
        for path, ssid, strength in APS:
            found[path] = {f"{NM}.AccessPoint": {
                "Ssid": v("ay", ssid.encode()), "Strength": v("y", strength), "Flags": v("u", 1),
                "WpaFlags": v("u", 0), "RsnFlags": v("u", 0x100), "Frequency": v("u", 5180),
            }}
    return found


def call(_c, _s, _p, _i, _method, _params, inv):
    inv.return_value(v("(a{oa{sa{sv}}})", (objects(),)))


def plug():
    # A placa de Wi-Fi espetada com o popup aberto: o sinal faz o popup reler
    state["wifi"] = True
    bus.emit_signal(None, "/org/freedesktop", "org.freedesktop.DBus.ObjectManager", "InterfacesAdded",
                    v("(oa{sa{sv}})", (WIFI, objects()[WIFI])))
    return GLib.SOURCE_CONTINUE


bus = Gio.bus_get_sync(Gio.BusType.SYSTEM)
bus.register_object("/org/freedesktop", Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], call)
Gio.bus_own_name_on_connection(bus, NM, 0, None, None)
GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, plug)
print("ready", flush=True)
GLib.MainLoop().run()

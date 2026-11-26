"""O AccountsService falso do headless-greeter.sh: o FindUserByName de cada conta e a propriedade Accent da
interface org.swaydesktop.Accounts. Uso: fake-accounts.py login=#rrggbb ... (num barramento que o
DBUS_SESSION_BUS_ADDRESS aponta; a tela o lê como o do sistema)."""
import sys

from gi.repository import Gio, GLib

XML = """<node>
<interface name="org.freedesktop.Accounts">
  <method name="FindUserByName"><arg type="s" direction="in"/><arg type="o" direction="out"/></method>
</interface>
<interface name="org.swaydesktop.Accounts"><property name="Accent" type="s" access="read"/></interface>
</node>"""
info = Gio.DBusNodeInfo.new_for_xml(XML)
accents = dict(arg.split("=", 1) for arg in sys.argv[1:])
bus = Gio.bus_get_sync(Gio.BusType.SESSION)


def find(_c, _s, _p, _i, _method, params, inv):
    login = params.unpack()[0]
    if login in accents:
        inv.return_value(GLib.Variant("(o)", (f"/org/freedesktop/Accounts/{login}",)))
    else:
        inv.return_dbus_error("org.freedesktop.Accounts.Error.Failed", f"no user {login}")


bus.register_object("/org/freedesktop/Accounts", info.interfaces[0], find)
for name, accent in accents.items():
    bus.register_object(f"/org/freedesktop/Accounts/{name}", info.interfaces[1], None,
                        lambda *_a, accent=accent: GLib.Variant("s", accent), None)
Gio.bus_own_name_on_connection(bus, "org.freedesktop.Accounts", 0, None, None)
print("ready", flush=True)
GLib.MainLoop().run()

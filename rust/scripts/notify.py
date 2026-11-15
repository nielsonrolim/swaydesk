"""Ajudante dos testes de toasts: manda uma notificação ou lista as da central, no barramento da sessão.

  notify.py notify RESUMO CORPO [--app A] [--critical] [--actions k1,Rótulo1,k2,Rótulo2] [--timeout MS]
  notify.py list | clear | dnd | set-dnd ESCOLHA
"""
import json
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

NAME = "org.freedesktop.Notifications"
PATH = "/org/freedesktop/Notifications"


def call(iface, method, params=None):
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    return bus.call_sync(NAME, PATH, iface, method, params, None, Gio.DBusCallFlags.NONE, 3000, None).unpack()


def main(argv):
    control = "local.SwayDesktop.Notifications1"
    if argv[0] in ("list", "dnd"):
        snap = json.loads(call(control, "List")[0])
        print(" ".join(str(i["id"]) for i in snap["items"]) if argv[0] == "list" else snap["dnd"]["choice"])
        return
    if argv[0] == "clear":
        call(control, "ClearAll")
        return
    if argv[0] == "set-dnd":
        call(control, "SetDnd", GLib.Variant("(s)", (argv[1],)))
        return
    summary, body, rest = argv[1], argv[2], argv[3:]
    app, hints, actions, timeout = "App", {}, [], -1
    while rest:
        flag = rest.pop(0)
        if flag == "--app":
            app = rest.pop(0)
        elif flag == "--critical":
            hints["urgency"] = GLib.Variant("y", 2)
        elif flag == "--actions":
            actions = rest.pop(0).split(",")
        elif flag == "--timeout":
            timeout = int(rest.pop(0))
    args = (app, 0, "dialog-information", summary, body, actions, hints, timeout)
    print(call("org.freedesktop.Notifications", "Notify", GLib.Variant("(susssasa{sv}i)", args))[0])


main(sys.argv[1:])

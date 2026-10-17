#!/usr/bin/env python3
"""A fake UPower and power-profiles-daemon for testing gaming mode (Phase 5a/5b)
without a laptop. Run it on a private bus and point the daemon at that bus:

    dbus-daemon --session --fork --print-address=1 > /tmp/bus     # any private bus
    DBUS_SYSTEM_BUS_ADDRESS=$(cat /tmp/bus) scripts/fake-upower.py [--desktop] [--battery] [--no-limit]
    DBUS_SYSTEM_BUS_ADDRESS=$(cat /tmp/bus) core/target/debug/somelauncherd

--desktop: no battery; --battery: start unplugged; --no-limit: a battery
without charge limit support. Plug in or out while it runs:

    busctl --address=$(cat /tmp/bus) call org.freedesktop.UPower /org/freedesktop/UPower org.somelauncher.FakeUPower SetOnBattery b true

Every change is printed, so the test can follow what the daemon did.
"""

import sys

from gi.repository import Gio, GLib

DESKTOP = "--desktop" in sys.argv
LIMIT = "--no-limit" not in sys.argv
BATTERY = "/org/freedesktop/UPower/devices/battery_BAT0"

state = {
    "on_battery": "--battery" in sys.argv,
    "limit": False,
    "profile": "balanced",
}

XML = """
<node>
  <interface name="org.freedesktop.UPower">
    <method name="EnumerateDevices"><arg type="ao" direction="out"/></method>
    <property name="OnBattery" type="b" access="read"/>
  </interface>
  <interface name="org.somelauncher.FakeUPower">
    <method name="SetOnBattery"><arg type="b" direction="in"/></method>
  </interface>
  <interface name="org.freedesktop.UPower.Device">
    <method name="EnableChargeThreshold"><arg type="b" direction="in"/></method>
    <property name="Type" type="u" access="read"/>
    <property name="PowerSupply" type="b" access="read"/>
    <property name="IsPresent" type="b" access="read"/>
    <property name="ChargeThresholdSupported" type="b" access="read"/>
    <property name="ChargeThresholdEnabled" type="b" access="read"/>
    <property name="ChargeStartThreshold" type="u" access="read"/>
    <property name="ChargeEndThreshold" type="u" access="read"/>
  </interface>
  <interface name="org.freedesktop.UPower.PowerProfiles">
    <property name="Profiles" type="aa{sv}" access="read"/>
    <property name="ActiveProfile" type="s" access="readwrite"/>
  </interface>
</node>
"""
node = Gio.DBusNodeInfo.new_for_xml(XML)


def changed(conn, path, iface, name, value):
    conn.emit_signal(None, path, "org.freedesktop.DBus.Properties", "PropertiesChanged",
                     GLib.Variant("(sa{sv}as)", (iface, {name: value}, [])))


def method(conn, sender, path, iface, name, params, inv):
    if name == "EnumerateDevices":
        inv.return_value(GLib.Variant("(ao)", ([] if DESKTOP else [BATTERY],)))
    elif name == "SetOnBattery":
        state["on_battery"] = params.unpack()[0]
        print("on battery:", state["on_battery"], flush=True)
        changed(conn, "/org/freedesktop/UPower", "org.freedesktop.UPower", "OnBattery", GLib.Variant("b", state["on_battery"]))
        inv.return_value(None)
    elif name == "EnableChargeThreshold":
        if not LIMIT:
            inv.return_dbus_error("org.freedesktop.UPower.GeneralError", "charge threshold is not supported")
            return
        state["limit"] = params.unpack()[0]
        print("charge limit:", state["limit"], flush=True)
        inv.return_value(None)


def get(conn, sender, path, iface, name):
    if iface == "org.freedesktop.UPower":
        return GLib.Variant("b", state["on_battery"])
    if iface == "org.freedesktop.UPower.PowerProfiles":
        if name == "ActiveProfile":
            return GLib.Variant("s", state["profile"])
        return GLib.Variant("aa{sv}", [{"Profile": GLib.Variant("s", p)} for p in ("power-saver", "balanced", "performance")])
    return {
        "Type": GLib.Variant("u", 2),
        "PowerSupply": GLib.Variant("b", True),
        "IsPresent": GLib.Variant("b", True),
        "ChargeThresholdSupported": GLib.Variant("b", LIMIT),
        "ChargeThresholdEnabled": GLib.Variant("b", state["limit"]),
        "ChargeStartThreshold": GLib.Variant("u", 0),
        "ChargeEndThreshold": GLib.Variant("u", 0),
    }[name]


def set_(conn, sender, path, iface, name, value):
    state["profile"] = value.unpack()
    print("profile:", state["profile"], flush=True)
    changed(conn, path, iface, name, value)
    return True


def interfaces(*names):
    return [i for i in node.interfaces if i.name in names]


def on_bus(conn, _name):
    for path, ifaces in (
        ("/org/freedesktop/UPower", interfaces("org.freedesktop.UPower", "org.somelauncher.FakeUPower")),
        (BATTERY, interfaces("org.freedesktop.UPower.Device")),
        ("/org/freedesktop/UPower/PowerProfiles", interfaces("org.freedesktop.UPower.PowerProfiles")),
    ):
        for i in ifaces:
            conn.register_object(path, i, method, get, set_)


Gio.bus_own_name(Gio.BusType.SYSTEM, "org.freedesktop.UPower", 0, on_bus, None, None)
Gio.bus_own_name(Gio.BusType.SYSTEM, "org.freedesktop.UPower.PowerProfiles", 0, None, None, None)
print("fake UPower ready" + (" (desktop)" if DESKTOP else ""), flush=True)
GLib.MainLoop().run()

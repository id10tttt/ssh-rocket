#!/usr/bin/env python3
from __future__ import annotations

import os
import signal
import sys

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

import gi
gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib

from helper.service import SshuttleService

BUS_NAME = "io.giggle.Sshuttle"


def on_bus_acquired(connection: Gio.DBusConnection, name: str, user_data: dict) -> None:
    """总线连接建立时实例化 D-Bus 服务并导出接口"""
    service = SshuttleService(connection)
    user_data["service"] = service


def on_name_acquired(connection: Gio.DBusConnection, name: str, user_data: dict) -> None:
    """成功接管目标总线名称"""
    pass


def on_name_lost(connection: Gio.DBusConnection, name: str, user_data: dict) -> None:
    """总线名称丢失或被其它进程抢占"""
    loop = user_data.get("loop")
    if loop:
        loop.quit()


def main() -> int:
    """D-Bus 守护服务入口"""
    loop = GLib.MainLoop()
    user_data = {"loop": loop, "service": None}

    owner_id = Gio.bus_own_name(
        Gio.BusType.SYSTEM,
        BUS_NAME,
        Gio.BusNameOwnerFlags.NONE,
        lambda conn, name: on_bus_acquired(conn, name, user_data),
        lambda conn, name: on_name_acquired(conn, name, user_data),
        lambda conn, name: on_name_lost(conn, name, user_data),
    )

    def sig_handler(*_: object) -> None:
        service = user_data.get("service")
        if service:
            service.stop_tunnel()
        Gio.bus_unown_name(owner_id)
        loop.quit()

    signal.signal(signal.SIGINT, sig_handler)
    signal.signal(signal.SIGTERM, sig_handler)

    try:
        loop.run()
    except KeyboardInterrupt:
        sig_handler()

    return 0


if __name__ == "__main__":
    sys.exit(main())

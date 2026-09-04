from __future__ import annotations

import sys
from typing import Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gio", "2.0")
from gi.repository import Adw, Gio, GLib, Gtk

from .config_manager import ConfigManager
from .tunnel_manager import TunnelManager
from .window import MainWindow


class SshuttleApplication(Adw.Application):
    """SShuttle 原生应用程序入口类"""

    def __init__(self) -> None:
        super().__init__(
            application_id="io.giggle.SshuttleGUI",
            flags=Gio.ApplicationFlags.DEFAULT_FLAGS,
        )
        self.config_manager = ConfigManager()
        self.tunnel_manager = TunnelManager(self.config_manager)
        self.window: Optional[MainWindow] = None

    def do_startup(self) -> None:
        """应用程序初始化生命周期回调"""
        Adw.Application.do_startup(self)

        # 注册动作
        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", lambda *_: self.quit())
        self.add_action(quit_action)

        about_action = Gio.SimpleAction.new("about", None)
        about_action.connect("activate", lambda *_: self._show_about())
        self.add_action(about_action)

        # 注册全局快捷键
        self.set_accels_for_action("app.quit", ["<Control>q"])
        self.set_accels_for_action("win.new-profile", ["<Control>n"])
        self.set_accels_for_action("win.show-logs", ["<Control>l"])

    def do_activate(self) -> None:
        """应用激活生命周期回调"""
        if not self.window:
            self.window = MainWindow(self, self.tunnel_manager)
        self.window.present()

    def _show_about(self) -> None:
        """展示原生关于对话框"""
        about = Adw.AboutWindow(
            transient_for=self.window,
            application_name="SShuttle",
            application_icon="network-vpn-symbolic",
            developer_name="Giggle",
            version="1.0.0",
            copyright="© 2026 Giggle",
        )
        about.present()

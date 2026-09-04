from __future__ import annotations

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk

from ..models import TunnelState
from ..tunnel_manager import TunnelManager


class StatusCard(Gtk.Box):
    """主界面顶部的连接状态核心概览与操作卡片"""

    def __init__(self, tunnel_manager: TunnelManager) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        self.tunnel_manager = tunnel_manager

        self.add_css_class("card")
        self.set_margin_top(12)
        self.set_margin_bottom(12)
        self.set_margin_start(16)
        self.set_margin_end(16)

        # 内部内边距容器
        inner_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        inner_box.set_margin_top(16)
        inner_box.set_margin_bottom(16)
        inner_box.set_margin_start(16)
        inner_box.set_margin_end(16)
        self.append(inner_box)

        # 状态指示条（状态圆点与文字）
        status_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.status_icon = Gtk.Image.new_from_icon_name("media-record-symbolic")
        self.status_icon.set_pixel_size(12)
        status_row.append(self.status_icon)

        self.status_label = Gtk.Label()
        self.status_label.add_css_class("title-4")
        status_row.append(self.status_label)
        inner_box.append(status_row)

        # Profile 概览信息
        info_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self.profile_name_label = Gtk.Label(xalign=0)
        self.profile_name_label.add_css_class("title-2")
        info_box.append(self.profile_name_label)

        self.summary_label = Gtk.Label(xalign=0)
        self.summary_label.add_css_class("dim-label")
        info_box.append(self.summary_label)
        inner_box.append(info_box)

        # 操作按钮与加载指示器容器
        btn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.action_button = Gtk.Button()
        self.action_button.set_hexpand(True)
        self.action_button.add_css_class("pill")
        self.action_button.connect("clicked", self._on_action_clicked)

        self.spinner = Gtk.Spinner()
        self.spinner.set_visible(False)

        btn_content = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        btn_content.set_halign(Gtk.Align.CENTER)
        self.btn_label = Gtk.Label()
        self.btn_label.add_css_class("title-4")
        btn_content.append(self.spinner)
        btn_content.append(self.btn_label)
        self.action_button.set_child(btn_content)

        btn_box.append(self.action_button)
        inner_box.append(btn_box)

        # 信号监听绑定
        self.tunnel_manager.connect("state-changed", lambda *_: self.update_view())
        self.tunnel_manager.connect("profile-changed", lambda *_: self.update_view())

        self.update_view()

    def _on_action_clicked(self, button: Gtk.Button) -> None:
        """主操作按钮点击"""
        self.tunnel_manager.toggle_connection()

    def update_view(self) -> None:
        """根据当前状态与 Profile 更新卡片视图展示"""
        state = self.tunnel_manager.state
        profile = self.tunnel_manager.active_profile

        # 清除按钮和图标的修饰类
        for css in ("suggested-action", "destructive-action", "accent", "success", "warning", "error"):
            self.action_button.remove_css_class(css)
            self.status_icon.remove_css_class(css)

        if profile:
            self.profile_name_label.set_text(profile.name)
            self.summary_label.set_text(profile.get_summary())
        else:
            self.profile_name_label.set_text("No Profile")
            self.summary_label.set_text("")

        if state == TunnelState.CONNECTED:
            self.status_label.set_text("Connected")
            self.status_icon.add_css_class("success")
            self.btn_label.set_text("Disconnect")
            self.action_button.add_css_class("destructive-action")
            self.action_button.set_sensitive(True)
            self.spinner.set_visible(False)
            self.spinner.stop()

        elif state == TunnelState.CONNECTING:
            self.status_label.set_text("Connecting")
            self.status_icon.add_css_class("warning")
            self.btn_label.set_text("Connecting")
            self.action_button.set_sensitive(False)
            self.spinner.set_visible(True)
            self.spinner.start()

        elif state == TunnelState.DISCONNECTING:
            self.status_label.set_text("Disconnecting")
            self.status_icon.add_css_class("warning")
            self.btn_label.set_text("Disconnecting")
            self.action_button.set_sensitive(False)
            self.spinner.set_visible(True)
            self.spinner.start()

        elif state == TunnelState.ERROR:
            self.status_label.set_text("Error")
            self.status_icon.add_css_class("error")
            self.btn_label.set_text("Reconnect")
            self.action_button.add_css_class("suggested-action")
            self.action_button.set_sensitive(bool(profile))
            self.spinner.set_visible(False)
            self.spinner.stop()

        else:  # DISCONNECTED
            self.status_label.set_text("Disconnected")
            self.status_icon.add_css_class("dim-label")
            self.btn_label.set_text("Connect")
            self.action_button.add_css_class("suggested-action")
            self.action_button.set_sensitive(bool(profile))
            self.spinner.set_visible(False)
            self.spinner.stop()

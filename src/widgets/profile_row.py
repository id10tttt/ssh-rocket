from __future__ import annotations

from typing import Callable, Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk

from ..models import Profile


class ProfileRow(Adw.ActionRow):
    """Profile 列表中展示单项配置的 ActionRow 组件"""

    def __init__(
        self,
        profile: Profile,
        is_active: bool,
        on_activated: Callable[[Profile], None],
        on_edit_clicked: Callable[[Profile], None],
    ) -> None:
        super().__init__()
        self.profile = profile
        self.on_activated = on_activated
        self.on_edit_clicked = on_edit_clicked

        self.set_title(profile.name)
        self.set_subtitle(profile.get_ssh_target())
        self.set_activatable(True)
        self.connect("activated", self._on_row_activated)

        # 激活状态图标
        self.active_icon = Gtk.Image.new_from_icon_name("object-select-symbolic")
        self.active_icon.set_visible(is_active)
        self.add_prefix(self.active_icon)

        # 编辑按钮
        self.edit_button = Gtk.Button.new_from_icon_name("go-next-symbolic")
        self.edit_button.add_css_class("flat")
        self.edit_button.set_valign(Gtk.Align.CENTER)
        self.edit_button.connect("clicked", self._on_edit)
        self.add_suffix(self.edit_button)

    def set_active_state(self, is_active: bool) -> None:
        """更新当前行是否为活跃项的状态展示"""
        self.active_icon.set_visible(is_active)

    def _on_row_activated(self, row: Adw.ActionRow) -> None:
        """整行被点击时设为当前活跃 Profile"""
        self.on_activated(self.profile)

    def _on_edit(self, button: Gtk.Button) -> None:
        """点击编辑按钮"""
        self.on_edit_clicked(self.profile)

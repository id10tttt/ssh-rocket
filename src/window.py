from __future__ import annotations

from typing import Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gio", "2.0")
from gi.repository import Adw, Gio, Gtk

from .models import Profile
from .tunnel_manager import TunnelManager
from .widgets.log_view import LogWindow
from .widgets.profile_editor import ProfileEditorWindow
from .widgets.profile_row import ProfileRow
from .widgets.status_card import StatusCard


class MainWindow(Adw.ApplicationWindow):
    """SShuttle 原生 GNOME 主应用程序窗口"""

    def __init__(self, app: Adw.Application, tunnel_manager: TunnelManager) -> None:
        super().__init__(application=app)
        self.tunnel_manager = tunnel_manager
        self.config_manager = tunnel_manager.config_manager

        # 还原窗口尺寸
        w = self.config_manager.get_setting("window_width", 440)
        h = self.config_manager.get_setting("window_height", 640)
        self.set_default_size(w, h)
        self.set_title("SShuttle")

        self._setup_actions()
        self._build_ui()

        # 监听 Profile 变更事件以同步刷新列表
        self.tunnel_manager.connect("profile-changed", lambda *_: self._refresh_profiles_list())
        self.connect("close-request", self._on_close_request)

    def _setup_actions(self) -> None:
        """注册窗口级别 GAction 操作与快捷键动作"""
        show_logs_action = Gio.SimpleAction.new("show-logs", None)
        show_logs_action.connect("activate", lambda *_: self.show_logs())
        self.add_action(show_logs_action)

        new_profile_action = Gio.SimpleAction.new("new-profile", None)
        new_profile_action.connect("activate", lambda *_: self._on_add_profile_clicked(None))
        self.add_action(new_profile_action)

    def _build_ui(self) -> None:
        """构建主窗口界面层次"""
        toolbar_view = Adw.ToolbarView()
        self.set_content(toolbar_view)

        # 顶部 HeaderBar
        header_bar = Adw.HeaderBar()
        toolbar_view.add_top_bar(header_bar)

        # 新增 Profile 按钮 (+)
        add_btn = Gtk.Button.new_from_icon_name("list-add-symbolic")
        add_btn.set_tooltip_text("Add Profile")
        add_btn.connect("clicked", self._on_add_profile_clicked)
        header_bar.pack_start(add_btn)

        # 右侧菜单 (Hamburger Menu)
        menu = Gio.Menu()
        menu.append("Logs", "win.show-logs")
        menu.append("About", "app.about")

        menu_btn = Gtk.MenuButton()
        menu_btn.set_icon_name("open-menu-symbolic")
        menu_btn.set_menu_model(menu)
        header_bar.pack_end(menu_btn)

        # 滚动区域与宽度限制器 (Clamp)
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        toolbar_view.set_content(scrolled)

        clamp = Adw.Clamp()
        clamp.set_maximum_size(520)
        clamp.set_tightening_threshold(380)
        scrolled.set_child(clamp)

        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        content_box.set_margin_top(16)
        content_box.set_margin_bottom(24)
        clamp.set_child(content_box)

        # 状态概览卡片
        self.status_card = StatusCard(self.tunnel_manager)
        content_box.append(self.status_card)

        # Profiles 分组
        self.profiles_group = Adw.PreferencesGroup(title="Profiles")
        self.profiles_group.set_margin_start(16)
        self.profiles_group.set_margin_end(16)
        content_box.append(self.profiles_group)

        self._refresh_profiles_list()

    def _refresh_profiles_list(self) -> None:
        """重新填充 Profile 列表项"""
        # 清除现有行
        while True:
            first = self.profiles_group.get_first_child()
            if not first:
                break
            self.profiles_group.remove(first)

        profiles = self.config_manager.get_profiles()
        active = self.config_manager.get_active_profile()
        active_id = active.id if active else None

        for p in profiles:
            is_active = (p.id == active_id)
            row = ProfileRow(
                profile=p,
                is_active=is_active,
                on_activated=self._on_select_profile,
                on_edit_clicked=self._on_edit_profile,
            )
            self.profiles_group.add(row)

        self.status_card.update_view()

    def _on_select_profile(self, profile: Profile) -> None:
        """选中某个 Profile 作为当前活跃项"""
        self.tunnel_manager.set_active_profile(profile.id)

    def _on_add_profile_clicked(self, button: Optional[Gtk.Button]) -> None:
        """打开新建 Profile 对话框"""
        editor = ProfileEditorWindow(
            profile=None,
            parent=self,
            on_save=self._on_profile_saved,
        )
        editor.present()

    def _on_edit_profile(self, profile: Profile) -> None:
        """打开编辑指定 Profile 对话框"""
        editor = ProfileEditorWindow(
            profile=profile,
            parent=self,
            on_save=self._on_profile_saved,
            on_delete=self._on_profile_deleted,
        )
        editor.present()

    def _on_profile_saved(self, profile: Profile) -> None:
        """保存 Profile 后的持久化及列表刷新"""
        self.config_manager.save_profile(profile)
        self._refresh_profiles_list()

    def _on_profile_deleted(self, profile: Profile) -> None:
        """删除 Profile 后的持久化及列表刷新"""
        self.config_manager.delete_profile(profile.id)
        self._refresh_profiles_list()

    def show_logs(self) -> None:
        """展示独立日志窗口"""
        log_win = LogWindow(self.tunnel_manager, self)
        log_win.present()

    def _on_close_request(self, window: Adw.ApplicationWindow) -> bool:
        """窗口关闭前持久化窗口尺寸"""
        w, h = self.get_default_size()
        self.config_manager.set_setting("window_width", w)
        self.config_manager.set_setting("window_height", h)
        return False

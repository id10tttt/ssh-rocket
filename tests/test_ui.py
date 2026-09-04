from __future__ import annotations

from pathlib import Path
import tempfile
import unittest

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gdk", "4.0")
from gi.repository import Adw, Gdk, Gtk

from src.config_manager import ConfigManager
from src.models import Profile
from src.tunnel_manager import TunnelManager
from src.widgets.log_view import LogWindow
from src.widgets.profile_editor import ProfileEditorWindow
from src.widgets.profile_row import ProfileRow
from src.widgets.status_card import StatusCard
from src.window import MainWindow


class TestSshuttleUI(unittest.TestCase):
    """GTK4 & Libadwaita UI 界面组件构建测试"""

    @classmethod
    def setUpClass(cls) -> None:
        """检查图形显示服务器可用性"""
        cls.has_display = False
        try:
            if Gtk.init_check():
                Adw.init()
                cls.has_display = Gdk.Display.get_default() is not None
        except Exception:
            cls.has_display = False

    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.config_manager = ConfigManager(config_dir=Path(self.temp_dir.name))
        self.tunnel_manager = TunnelManager(self.config_manager)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_ui_classes_importable(self) -> None:
        """测试所有 UI 核心组件类正常导入且类属性完备"""
        self.assertTrue(issubclass(StatusCard, Gtk.Box))
        self.assertTrue(issubclass(ProfileRow, Adw.ActionRow))
        self.assertTrue(issubclass(ProfileEditorWindow, Adw.PreferencesWindow))
        self.assertTrue(issubclass(LogWindow, Adw.Window))
        self.assertTrue(issubclass(MainWindow, Adw.ApplicationWindow))

    def test_status_card_creation(self) -> None:
        """测试 StatusCard 组件初始化与视图更新"""
        if not self.has_display:
            self.skipTest("No display available in headless environment")
        card = StatusCard(self.tunnel_manager)
        self.assertIsNotNone(card)
        self.assertEqual(card.status_label.get_text(), "Disconnected")

    def test_profile_row_creation(self) -> None:
        """测试 ProfileRow 列表项组件"""
        if not self.has_display:
            self.skipTest("No display available in headless environment")
        p = Profile(name="Node A", host="10.0.0.1")
        row = ProfileRow(
            profile=p,
            is_active=True,
            on_activated=lambda *_: None,
            on_edit_clicked=lambda *_: None,
        )
        self.assertIsNotNone(row)
        self.assertEqual(row.get_title(), "Node A")
        self.assertEqual(row.get_subtitle(), "10.0.0.1")

    def test_profile_editor_window(self) -> None:
        """测试 Profile 编辑窗口组件"""
        if not self.has_display:
            self.skipTest("No display available in headless environment")
        p = Profile(name="Node B", host="10.0.0.2")
        editor = ProfileEditorWindow(profile=p)
        self.assertIsNotNone(editor)
        self.assertEqual(editor.name_row.get_text(), "Node B")
        self.assertEqual(editor.host_row.get_text(), "10.0.0.2")

    def test_log_window_creation(self) -> None:
        """测试 LogWindow 日志窗口组件"""
        if not self.has_display:
            self.skipTest("No display available in headless environment")
        dummy_parent = Gtk.Window()
        log_win = LogWindow(self.tunnel_manager, dummy_parent)
        self.assertIsNotNone(log_win)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gdk", "4.0")
from gi.repository import Adw, Gdk, Gtk

from ..tunnel_manager import TunnelManager


class LogWindow(Adw.Window):
    """独立的实时日志查看窗口"""

    def __init__(self, tunnel_manager: TunnelManager, parent: Gtk.Window) -> None:
        super().__init__()
        self.tunnel_manager = tunnel_manager
        self.set_transient_for(parent)
        self.set_default_size(580, 420)
        self.set_title("Logs")

        # 整体布局
        toolbar_view = Adw.ToolbarView()
        self.set_content(toolbar_view)

        header_bar = Adw.HeaderBar()
        toolbar_view.add_top_bar(header_bar)

        # 复制日志按钮
        copy_btn = Gtk.Button.new_from_icon_name("edit-copy-symbolic")
        copy_btn.set_tooltip_text("Copy")
        copy_btn.connect("clicked", self._on_copy_clicked)
        header_bar.pack_start(copy_btn)

        # 清空日志按钮
        clear_btn = Gtk.Button.new_from_icon_name("edit-clear-all-symbolic")
        clear_btn.set_tooltip_text("Clear")
        clear_btn.connect("clicked", self._on_clear_clicked)
        header_bar.pack_start(clear_btn)

        # 文本视图与滚动窗口
        self.scrolled_window = Gtk.ScrolledWindow()
        self.text_view = Gtk.TextView()
        self.text_view.set_editable(False)
        self.text_view.set_cursor_visible(False)
        self.text_view.set_monospace(True)
        self.text_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.text_view.set_top_margin(12)
        self.text_view.set_bottom_margin(12)
        self.text_view.set_left_margin(12)
        self.text_view.set_right_margin(12)

        self.scrolled_window.set_child(self.text_view)
        toolbar_view.set_content(self.scrolled_window)

        # 初始化已有历史日志
        self.buffer = self.text_view.get_buffer()
        if self.tunnel_manager.log_history:
            self.buffer.set_text("\n".join(self.tunnel_manager.log_history) + "\n")
            self._scroll_to_bottom()

        # 绑定日志接收信号
        self._handler_id = self.tunnel_manager.connect("log-received", self._on_log_received)
        self.connect("close-request", self._on_close_request)

    def _on_log_received(self, manager: TunnelManager, line: str) -> None:
        """接收并追加新日志行"""
        end_iter = self.buffer.get_end_iter()
        self.buffer.insert(end_iter, line + "\n")
        self._scroll_to_bottom()

    def _scroll_to_bottom(self) -> None:
        """平滑滚动到底部"""
        end_iter = self.buffer.get_end_iter()
        mark = self.buffer.create_mark(None, end_iter, False)
        self.text_view.scroll_to_mark(mark, 0.0, True, 0.0, 1.0)

    def _on_copy_clicked(self, button: Gtk.Button) -> None:
        """复制所有日志内容到剪贴板"""
        start_iter = self.buffer.get_start_iter()
        end_iter = self.buffer.get_end_iter()
        text = self.buffer.get_text(start_iter, end_iter, False)
        display = Gdk.Display.get_default()
        if display:
            display.get_clipboard().set(text)

    def _on_clear_clicked(self, button: Gtk.Button) -> None:
        """清空日志缓存与界面显示"""
        self.tunnel_manager.clear_logs()
        self.buffer.set_text("")

    def _on_close_request(self, window: Adw.Window) -> bool:
        """窗口关闭时解除信号绑定"""
        if self._handler_id:
            self.tunnel_manager.disconnect(self._handler_id)
            self._handler_id = None
        return False

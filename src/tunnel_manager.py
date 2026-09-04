from __future__ import annotations

from typing import List, Optional

import gi
gi.require_version("GObject", "2.0")
from gi.repository import GObject

from .config_manager import ConfigManager
from .dbus_client import DBusTunnelBackend
from .models import Profile, TunnelState
from .tunnel_backend import DirectSubprocessBackend, TunnelBackend


class TunnelManager(GObject.Object):
    """隧道中介控制中心，协调 Profile 配置管理、后端状态同步及日志缓存"""

    __gsignals__ = {
        "state-changed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "log-received": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "profile-changed": (GObject.SignalFlags.RUN_FIRST, None, ()),
    }

    def __init__(self, config_manager: ConfigManager) -> None:
        super().__init__()
        self.config_manager = config_manager
        self.log_history: List[str] = []
        self._max_logs: int = 1000

        # 初始化后端（优先尝试 D-Bus，失败则降级到直连子进程）
        self.backend: TunnelBackend = self._create_backend()
        self._bind_backend()

    def _create_backend(self) -> TunnelBackend:
        """根据系统环境与可用性选择最优后端"""
        preferred = self.config_manager.get_setting("backend_mode", "auto")
        if preferred == "direct":
            return DirectSubprocessBackend()

        dbus_backend = DBusTunnelBackend()
        if dbus_backend.is_available or preferred == "dbus":
            return dbus_backend

        return DirectSubprocessBackend()

    def _bind_backend(self) -> None:
        """绑定后端的事件信号"""
        self.backend.connect("state-changed", self._on_backend_state_changed)
        self.backend.connect("log-received", self._on_backend_log_received)

    def _on_backend_state_changed(self, backend: TunnelBackend, state_str: str) -> None:
        """中继后端状态变更信号"""
        self.emit("state-changed", state_str)

    def _on_backend_log_received(self, backend: TunnelBackend, log_line: str) -> None:
        """缓存日志历史并中继日志信号"""
        self.log_history.append(log_line)
        if len(self.log_history) > self._max_logs:
            self.log_history.pop(0)
        self.emit("log-received", log_line)

    @property
    def state(self) -> TunnelState:
        """获取当前连接状态"""
        return self.backend.state

    @property
    def active_profile(self) -> Optional[Profile]:
        """获取当前活跃的 Profile 配置"""
        return self.config_manager.get_active_profile()

    def set_active_profile(self, profile_id: str) -> None:
        """切换活跃 Profile"""
        self.config_manager.set_active_profile(profile_id)
        self.emit("profile-changed")

    def connect_active(self) -> None:
        """连接当前选中的活跃 Profile"""
        profile = self.active_profile
        if not profile:
            self.emit("log-received", "No profile selected to connect.")
            return
        self.backend.start(profile)

    def disconnect(self) -> None:
        """断开当前连接"""
        self.backend.stop()

    def toggle_connection(self) -> None:
        """一键切换连接/断开状态"""
        if self.state in (TunnelState.CONNECTED, TunnelState.CONNECTING):
            self.disconnect()
        else:
            self.connect_active()

    def clear_logs(self) -> None:
        """清空日志缓存"""
        self.log_history.clear()

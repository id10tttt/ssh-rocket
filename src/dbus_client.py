from __future__ import annotations

import json
from typing import Optional

import gi
gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib

from .models import Profile, TunnelState
from .tunnel_backend import TunnelBackend

BUS_NAME = "io.giggle.Sshuttle"
OBJECT_PATH = "/io/giggle/Sshuttle"
INTERFACE_NAME = "io.giggle.Sshuttle.Manager"


class DBusTunnelBackend(TunnelBackend):
    """基于系统 D-Bus 总线连接后台 systemd helper 的客户端实现"""

    def __init__(self) -> None:
        super().__init__()
        self._proxy: Optional[Gio.DBusProxy] = None
        self._available: bool = False
        self._init_dbus()

    @property
    def is_available(self) -> bool:
        """获取 D-Bus 后台服务是否处于可用就绪状态"""
        return self._available

    def _init_dbus(self) -> None:
        """尝试连接系统总线中的目标 D-Bus 服务代理"""
        try:
            self._proxy = Gio.DBusProxy.new_for_bus_sync(
                Gio.BusType.SYSTEM,
                Gio.DBusProxyFlags.NONE,
                None,
                BUS_NAME,
                OBJECT_PATH,
                INTERFACE_NAME,
                None,
            )

            # 校验远端对象或名称是否存在持有者
            owner = self._proxy.get_name_owner()
            if owner:
                self._available = True
                self._proxy.connect("g-signal", self._on_signal)
                self._sync_status()
            else:
                self._available = False
        except Exception:
            self._available = False

    def _sync_status(self) -> None:
        """从后台服务主动同步当前连接状态"""
        if not self._proxy:
            return
        try:
            res = self._proxy.call_sync(
                "GetStatus",
                None,
                Gio.DBusCallFlags.NONE,
                1000,
                None,
            )
            status_str = res.unpack()[0]
            try:
                self._set_state(TunnelState(status_str))
            except ValueError:
                self._set_state(TunnelState.DISCONNECTED)
        except Exception:
            pass

    def _on_signal(
        self,
        proxy: Gio.DBusProxy,
        sender_name: str,
        signal_name: str,
        parameters: GLib.Variant,
    ) -> None:
        """分发来自 D-Bus 后台服务的状态和日志信号"""
        if signal_name == "StatusChanged":
            status_str = parameters.unpack()[0]
            try:
                self._set_state(TunnelState(status_str))
            except ValueError:
                pass
        elif signal_name == "LogReceived":
            line = parameters.unpack()[0]
            self._emit_log(line)
        elif signal_name == "ProcessExited":
            code = parameters.unpack()[0]
            self._emit_log(f"Remote process exited with code {code}")

    def start(self, profile: Profile) -> None:
        """通过 D-Bus 接口请求启动隧道"""
        if not self._proxy or not self._available:
            self._emit_log("D-Bus service not available.")
            self._set_state(TunnelState.ERROR)
            return

        self._active_profile = profile
        self._set_state(TunnelState.CONNECTING)
        payload = json.dumps(profile.to_dict())

        try:
            self._proxy.call(
                "Start",
                GLib.Variant("(s)", (payload,)),
                Gio.DBusCallFlags.NONE,
                -1,
                None,
                self._on_call_finish,
            )
        except Exception as e:
            self._emit_log(f"Failed to call Start via D-Bus: {e}")
            self._set_state(TunnelState.ERROR)

    def stop(self) -> None:
        """通过 D-Bus 接口请求停止隧道"""
        if not self._proxy or not self._available:
            self._set_state(TunnelState.DISCONNECTED)
            return

        self._set_state(TunnelState.DISCONNECTING)
        try:
            self._proxy.call(
                "Stop",
                None,
                Gio.DBusCallFlags.NONE,
                -1,
                None,
                self._on_call_finish,
            )
        except Exception as e:
            self._emit_log(f"Failed to call Stop via D-Bus: {e}")
            self._set_state(TunnelState.DISCONNECTED)

    def _on_call_finish(
        self,
        proxy: Gio.DBusProxy,
        result: Gio.AsyncResult,
    ) -> None:
        """异步调用完成回调"""
        try:
            proxy.call_finish(result)
        except Exception as e:
            self._emit_log(f"D-Bus call error: {e}")

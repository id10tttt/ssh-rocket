from __future__ import annotations

import os
import signal
from typing import Optional

import gi
gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib, GObject

from .command_builder import CommandBuilder
from .models import Profile, TunnelState


class TunnelBackend(GObject.Object):
    """隧道后端抽象基类，定义状态变更与日志通知信号"""

    __gsignals__ = {
        "state-changed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "log-received": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self) -> None:
        super().__init__()
        self._state: TunnelState = TunnelState.DISCONNECTED
        self._active_profile: Optional[Profile] = None

    @property
    def state(self) -> TunnelState:
        """获取当前隧道状态"""
        return self._state

    @property
    def active_profile(self) -> Optional[Profile]:
        """获取当前运行的 Profile"""
        return self._active_profile

    def _set_state(self, new_state: TunnelState) -> None:
        """更新状态并触发 GObject 信号"""
        if self._state != new_state:
            self._state = new_state
            self.emit("state-changed", new_state.value)

    def _emit_log(self, text: str) -> None:
        """向监听者分发日志行"""
        self.emit("log-received", text)

    def start(self, profile: Profile) -> None:
        """启动隧道连接"""
        raise NotImplementedError

    def stop(self) -> None:
        """停止隧道连接"""
        raise NotImplementedError


class DirectSubprocessBackend(TunnelBackend):
    """基于 Gio.Subprocess 的本地直连后端实现"""

    def __init__(self, use_pkexec: Optional[bool] = None) -> None:
        super().__init__()
        # 非 root 用户下默认使用 pkexec 进行权限提权
        if use_pkexec is None:
            self.use_pkexec = os.geteuid() != 0
        else:
            self.use_pkexec = use_pkexec

        self._process: Optional[Gio.Subprocess] = None
        self._cancellable: Optional[Gio.Cancellable] = None
        self._connect_timer_id: Optional[int] = None

    def start(self, profile: Profile) -> None:
        """根据 Profile 启动 sshuttle 进程"""
        if self._state in (TunnelState.CONNECTING, TunnelState.CONNECTED):
            return

        self._active_profile = profile
        self._set_state(TunnelState.CONNECTING)

        try:
            argv = CommandBuilder.build_argv(profile, use_pkexec=self.use_pkexec)
            self._emit_log(f"Starting tunnel: {' '.join(argv)}")

            flags = (
                Gio.SubprocessFlags.STDOUT_PIPE
                | Gio.SubprocessFlags.STDERR_PIPE
            )
            self._cancellable = Gio.Cancellable.new()
            self._process = Gio.Subprocess.new(argv, flags)

            # 异步读取 stdout 与 stderr
            stdout_pipe = self._process.get_stdout_pipe()
            stderr_pipe = self._process.get_stderr_pipe()

            if stdout_pipe:
                self._read_stream(stdout_pipe)
            if stderr_pipe:
                self._read_stream(stderr_pipe)

            # 监听进程结束
            self._process.wait_async(self._cancellable, self._on_process_exit)

            # 设置 4 秒兜底定时器：若进程未退出且未报错，假定连接已建立
            self._connect_timer_id = GLib.timeout_add_seconds(4, self._on_connect_timeout)

        except Exception as e:
            self._emit_log(f"Error starting process: {e}")
            self._set_state(TunnelState.ERROR)

    def _read_stream(self, stream: Gio.InputStream) -> None:
        """异步按行读取输出流数据"""
        data_stream = Gio.DataInputStream.new(stream)

        def on_line_ready(source: Gio.DataInputStream, result: Gio.AsyncResult) -> None:
            try:
                line_data, _ = source.read_line_finish_utf8(result)
                if line_data is not None:
                    self._on_log_line(line_data)
                    if self._cancellable and not self._cancellable.is_cancelled():
                        source.read_line_async(
                            GLib.PRIORITY_DEFAULT,
                            self._cancellable,
                            on_line_ready,
                        )
            except Exception:
                pass

        data_stream.read_line_async(
            GLib.PRIORITY_DEFAULT,
            self._cancellable,
            on_line_ready,
        )

    def _on_log_line(self, line: str) -> None:
        """处理日志行，并根据关键标记识别连接建立"""
        self._emit_log(line)

        lower_line = line.lower()
        if "connected" in lower_line or "tunnel ready" in lower_line or "c : connected" in lower_line:
            if self._state == TunnelState.CONNECTING:
                if self._connect_timer_id:
                    GLib.source_remove(self._connect_timer_id)
                    self._connect_timer_id = None
                self._set_state(TunnelState.CONNECTED)

    def _on_connect_timeout(self) -> bool:
        """连接启动超时检查，如进程健康运行则标记为已连接"""
        self._connect_timer_id = None
        if self._state == TunnelState.CONNECTING and self._process:
            self._set_state(TunnelState.CONNECTED)
        return False

    def stop(self) -> None:
        """停止运行中的 sshuttle 进程"""
        if self._state in (TunnelState.DISCONNECTED, TunnelState.DISCONNECTING):
            return

        if self._connect_timer_id:
            GLib.source_remove(self._connect_timer_id)
            self._connect_timer_id = None

        self._set_state(TunnelState.DISCONNECTING)
        self._emit_log("Disconnecting tunnel...")

        if self._process:
            try:
                self._process.send_signal(signal.SIGINT)
                GLib.timeout_add_seconds(3, self._force_kill_if_needed)
            except Exception as e:
                self._emit_log(f"Failed to send SIGINT: {e}")
                self._set_state(TunnelState.DISCONNECTED)
        else:
            self._set_state(TunnelState.DISCONNECTED)

    def _force_kill_if_needed(self) -> bool:
        """若优雅退出超时，强制终止进程"""
        if self._state == TunnelState.DISCONNECTING and self._process:
            try:
                self._process.force_exit()
            except Exception:
                pass
        return False

    def _on_process_exit(self, process: Gio.Subprocess, result: Gio.AsyncResult) -> None:
        """进程退出回调处理"""
        if self._connect_timer_id:
            GLib.source_remove(self._connect_timer_id)
            self._connect_timer_id = None

        if self._cancellable:
            self._cancellable.cancel()
            self._cancellable = None

        try:
            process.wait_finish(result)
            exit_code = process.get_exit_status()
            self._emit_log(f"Process exited with code {exit_code}")
        except Exception as e:
            self._emit_log(f"Process wait error: {e}")

        self._process = None

        if self._state == TunnelState.DISCONNECTING:
            self._set_state(TunnelState.DISCONNECTED)
        elif self._state != TunnelState.DISCONNECTED:
            self._set_state(TunnelState.ERROR)

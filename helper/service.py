from __future__ import annotations

import json
import signal
from typing import Optional

import gi
gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib

from src.command_builder import CommandBuilder
from src.models import Profile, TunnelState

INTROSPECTION_XML = """
<node>
  <interface name='io.giggle.Sshuttle.Manager'>
    <method name='Start'>
      <arg type='s' name='profile_json' direction='in'/>
      <arg type='b' name='success' direction='out'/>
    </method>
    <method name='Stop'>
      <arg type='b' name='success' direction='out'/>
    </method>
    <method name='Restart'>
      <arg type='s' name='profile_json' direction='in'/>
      <arg type='b' name='success' direction='out'/>
    </method>
    <method name='GetStatus'>
      <arg type='s' name='status' direction='out'/>
    </method>
    <method name='GetActiveProfile'>
      <arg type='s' name='profile_json' direction='out'/>
    </method>
    <signal name='StatusChanged'>
      <arg type='s' name='status'/>
    </signal>
    <signal name='LogReceived'>
      <arg type='s' name='log_line'/>
    </signal>
    <signal name='ProcessExited'>
      <arg type='i' name='exit_code'/>
    </signal>
  </interface>
</node>
"""


class SshuttleService:
    """后台系统服务实现，负责直接执行特权 sshuttle 进程并通过 D-Bus 暴露控制接口"""

    def __init__(self, connection: Gio.DBusConnection) -> None:
        self.connection = connection
        self.state: TunnelState = TunnelState.DISCONNECTED
        self.active_profile: Optional[Profile] = None

        self._process: Optional[Gio.Subprocess] = None
        self._cancellable: Optional[Gio.Cancellable] = None
        self._connect_timer_id: Optional[int] = None

        # 注册 D-Bus 对象
        node_info = Gio.DBusNodeInfo.new_for_xml(INTROSPECTION_XML)
        self.interface_info = node_info.interfaces[0]

        self.registration_id = self.connection.register_object(
            "/io/giggle/Sshuttle",
            self.interface_info,
            self._handle_method_call,
            None,
            None,
        )

    def _emit_signal(self, signal_name: str, parameters: GLib.Variant) -> None:
        """向系统总线广播 D-Bus 信号"""
        try:
            self.connection.emit_signal(
                None,
                "/io/giggle/Sshuttle",
                "io.giggle.Sshuttle.Manager",
                signal_name,
                parameters,
            )
        except Exception:
            pass

    def _set_state(self, new_state: TunnelState) -> None:
        """变更服务状态并广播通知"""
        if self.state != new_state:
            self.state = new_state
            self._emit_signal("StatusChanged", GLib.Variant("(s)", (new_state.value,)))

    def _emit_log(self, text: str) -> None:
        """广播日志行"""
        self._emit_signal("LogReceived", GLib.Variant("(s)", (text,)))

    def _handle_method_call(
        self,
        connection: Gio.DBusConnection,
        sender: str,
        object_path: str,
        interface_name: str,
        method_name: str,
        parameters: GLib.Variant,
        invocation: Gio.DBusMethodInvocation,
    ) -> None:
        """处理来自 D-Bus 客户端的 RPC 方法调用"""
        if method_name == "Start":
            profile_json = parameters.unpack()[0]
            try:
                profile_dict = json.loads(profile_json)
                profile = Profile.from_dict(profile_dict)
                success = self.start_tunnel(profile)
                invocation.return_value(GLib.Variant("(b)", (success,)))
            except Exception as e:
                self._emit_log(f"Invalid profile or start failure: {e}")
                invocation.return_value(GLib.Variant("(b)", (False,)))

        elif method_name == "Stop":
            success = self.stop_tunnel()
            invocation.return_value(GLib.Variant("(b)", (success,)))

        elif method_name == "Restart":
            profile_json = parameters.unpack()[0]
            try:
                profile_dict = json.loads(profile_json)
                profile = Profile.from_dict(profile_dict)
                self.stop_tunnel()
                success = self.start_tunnel(profile)
                invocation.return_value(GLib.Variant("(b)", (success,)))
            except Exception:
                invocation.return_value(GLib.Variant("(b)", (False,)))

        elif method_name == "GetStatus":
            invocation.return_value(GLib.Variant("(s)", (self.state.value,)))

        elif method_name == "GetActiveProfile":
            res = json.dumps(self.active_profile.to_dict()) if self.active_profile else ""
            invocation.return_value(GLib.Variant("(s)", (res,)))

        else:
            invocation.return_error_literal(
                Gio.DBusError.quark(),
                Gio.DBusError.UNKNOWN_METHOD,
                f"Unknown method {method_name}",
            )

    def start_tunnel(self, profile: Profile) -> bool:
        """根据 Profile 启动 sshuttle 进程"""
        if self.state in (TunnelState.CONNECTED, TunnelState.CONNECTING):
            return False

        self.active_profile = profile
        self._set_state(TunnelState.CONNECTING)

        try:
            argv = CommandBuilder.build_argv(profile, use_pkexec=False)
            self._emit_log(f"Service starting: {' '.join(argv)}")

            flags = (
                Gio.SubprocessFlags.STDOUT_PIPE
                | Gio.SubprocessFlags.STDERR_PIPE
            )
            self._cancellable = Gio.Cancellable.new()
            self._process = Gio.Subprocess.new(argv, flags)

            stdout_pipe = self._process.get_stdout_pipe()
            stderr_pipe = self._process.get_stderr_pipe()

            if stdout_pipe:
                self._read_stream(stdout_pipe)
            if stderr_pipe:
                self._read_stream(stderr_pipe)

            self._process.wait_async(self._cancellable, self._on_process_exit)
            self._connect_timer_id = GLib.timeout_add_seconds(4, self._on_connect_timeout)
            return True

        except Exception as e:
            self._emit_log(f"Failed to spawn sshuttle: {e}")
            self._set_state(TunnelState.ERROR)
            return False

    def _read_stream(self, stream: Gio.InputStream) -> None:
        """异步按行流式读取日志输出"""
        data_stream = Gio.DataInputStream.new(stream)

        def on_line(source: Gio.DataInputStream, result: Gio.AsyncResult) -> None:
            try:
                line_data, _ = source.read_line_finish_utf8(result)
                if line_data is not None:
                    self._on_log_line(line_data)
                    if self._cancellable and not self._cancellable.is_cancelled():
                        source.read_line_async(
                            GLib.PRIORITY_DEFAULT,
                            self._cancellable,
                            on_line,
                        )
            except Exception:
                pass

        data_stream.read_line_async(GLib.PRIORITY_DEFAULT, self._cancellable, on_line)

    def _on_log_line(self, line: str) -> None:
        """日志处理并探测就绪状态"""
        self._emit_log(line)
        lower = line.lower()
        if "connected" in lower or "tunnel ready" in lower:
            if self.state == TunnelState.CONNECTING:
                if self._connect_timer_id:
                    GLib.source_remove(self._connect_timer_id)
                    self._connect_timer_id = None
                self._set_state(TunnelState.CONNECTED)

    def _on_connect_timeout(self) -> bool:
        """就绪超时判定"""
        self._connect_timer_id = None
        if self.state == TunnelState.CONNECTING and self._process:
            self._set_state(TunnelState.CONNECTED)
        return False

    def stop_tunnel(self) -> bool:
        """终止当前运行中的 sshuttle 进程"""
        if self.state in (TunnelState.DISCONNECTED, TunnelState.DISCONNECTING):
            return True

        if self._connect_timer_id:
            GLib.source_remove(self._connect_timer_id)
            self._connect_timer_id = None

        self._set_state(TunnelState.DISCONNECTING)
        self._emit_log("Service stopping tunnel...")

        if self._process:
            try:
                self._process.send_signal(signal.SIGINT)
                GLib.timeout_add_seconds(3, self._force_kill_if_needed)
            except Exception as e:
                self._emit_log(f"SIGINT send failed: {e}")
                self._set_state(TunnelState.DISCONNECTED)
        else:
            self._set_state(TunnelState.DISCONNECTED)
        return True

    def _force_kill_if_needed(self) -> bool:
        """强制杀死超时未退出的进程"""
        if self.state == TunnelState.DISCONNECTING and self._process:
            try:
                self._process.force_exit()
            except Exception:
                pass
        return False

    def _on_process_exit(self, process: Gio.Subprocess, result: Gio.AsyncResult) -> None:
        """进程退出处理"""
        if self._connect_timer_id:
            GLib.source_remove(self._connect_timer_id)
            self._connect_timer_id = None

        if self._cancellable:
            self._cancellable.cancel()
            self._cancellable = None

        exit_code = 0
        try:
            process.wait_finish(result)
            exit_code = process.get_exit_status()
            self._emit_log(f"Backend process terminated, exit code {exit_code}")
        except Exception as e:
            self._emit_log(f"Process wait finish failed: {e}")

        self._process = None
        self._emit_signal("ProcessExited", GLib.Variant("(i)", (exit_code,)))

        if self.state == TunnelState.DISCONNECTING:
            self._set_state(TunnelState.DISCONNECTED)
        elif self.state != TunnelState.DISCONNECTED:
            self._set_state(TunnelState.ERROR)

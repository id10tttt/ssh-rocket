namespace Sshuttle {

    public class TunnelManager : Object {
        public signal void state_changed (TunnelState state);
        public signal void log_received (string line);
        public signal void profile_changed ();

        public ConfigManager config_manager { get; private set; }
        public TunnelState state { get; private set; default = TunnelState.DISCONNECTED; }
        public GLib.GenericArray<string> log_history { get; private set; }

        public CgroupManager cgroup_manager { get; private set; }
        public NftManager nft_manager { get; private set; }
        public ProcessMonitor process_monitor { get; private set; }
        public int local_proxy_port { get; set; default = 12300; }

        public int reconnect_attempt { get; private set; default = 0; }
        private uint reconnect_timeout_id = 0;
        private const uint BASE_RECONNECT_DELAY = 2;
        private const uint MAX_RECONNECT_DELAY = 30;

        private GLib.Subprocess? process = null;
        private GLib.Cancellable? cancellable = null;
        private uint connect_timeout_id = 0;
        private const uint MAX_LOGS = 1000;

        public Profile? active_profile {
            owned get {
                return this.config_manager.get_active_profile ();
            }
        }

        public TunnelManager (ConfigManager config_manager) {
            this.config_manager = config_manager;
            this.log_history = new GLib.GenericArray<string> ();

            this.cgroup_manager = new CgroupManager ();
            this.nft_manager = new NftManager ();
            this.process_monitor = new ProcessMonitor (this.cgroup_manager);

            // 启动时主动清除任何可能的上次异常残留（确保纯运行时无残余）
            this.cleanup_proxy_runtime ();

            this.config_manager.app_rules_changed.connect (this.on_app_rules_changed);
        }

        public void set_active_profile (string id) {
            this.config_manager.set_active_profile (id);
            this.profile_changed ();
        }

        private void change_state (TunnelState new_state) {
            if (this.state != new_state) {
                this.state = new_state;
                this.state_changed (new_state);
            }
        }

        private void emit_log (string text) {
            this.log_history.add (text);
            if (this.log_history.length > MAX_LOGS) {
                this.log_history.remove_index (0);
            }
            this.log_received (text);
            print ("%s\n", text);
        }

        public void clear_logs () {
            this.log_history.remove_range (0, this.log_history.length);
        }

        public void toggle_connection () {
            if (this.state == TunnelState.CONNECTED || this.state == TunnelState.CONNECTING) {
                this.disconnect_tunnel ();
            } else {
                this.connect_active ();
            }
        }

        public void connect_active () {
            var profile = this.active_profile;
            if (profile == null) {
                this.emit_log ("No profile selected to connect.");
                return;
            }
            this.start_tunnel (profile);
        }

        public void start_tunnel (Profile profile) {
            if (this.state == TunnelState.CONNECTING || this.state == TunnelState.CONNECTED) {
                return;
            }

            // 取消正在等待的自动重连倒计时
            this.cancel_reconnect ();

            this.change_state (TunnelState.CONNECTING);

            try {
                // 启动按软件代理监控
                this.sync_process_monitor_targets ();
                this.process_monitor.start ();

                string[] argv = CommandBuilder.build_argv (profile, this.local_proxy_port);

                string cmd_str = string.joinv (" ", argv);
                this.emit_log (@"Starting tunnel: $(cmd_str)");

                this.cancellable = new GLib.Cancellable ();
                var launcher = new GLib.SubprocessLauncher (
                    GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE
                );

                this.process = launcher.spawnv (argv);

                var stdout_pipe = this.process.get_stdout_pipe ();
                var stderr_pipe = this.process.get_stderr_pipe ();

                if (stdout_pipe != null) {
                    this.read_stream_async.begin (stdout_pipe);
                }
                if (stderr_pipe != null) {
                    this.read_stream_async.begin (stderr_pipe);
                }

                this.wait_process_async.begin ();

                this.connect_timeout_id = GLib.Timeout.add_seconds (25, () => {
                    this.connect_timeout_id = 0;
                    if (this.state == TunnelState.CONNECTING) {
                        this.emit_log ("Connection timed out after 25 seconds.");
                        this.disconnect_tunnel ();
                        this.change_state (TunnelState.ERROR);
                        this.schedule_auto_reconnect ();
                    }
                    return false;
                });

            } catch (GLib.Error e) {
                this.emit_log (@"Failed to spawn sshuttle: $(e.message)");
                this.cleanup_proxy_runtime ();
                this.change_state (TunnelState.ERROR);
                this.schedule_auto_reconnect ();
            }
        }

        private async void wait_process_async () {
            try {
                if (this.process != null) {
                    yield this.process.wait_async (this.cancellable);
                }
            } catch (GLib.Error e) {
                this.emit_log (@"Process wait failed: $(e.message)");
            }
            this.on_process_exit ();
        }

        private async void read_stream_async (GLib.InputStream stream) {
            var data_stream = new GLib.DataInputStream (stream);
            try {
                while (true) {
                    size_t length;
                    string? line = yield data_stream.read_line_utf8_async (
                        GLib.Priority.DEFAULT,
                        this.cancellable,
                        out length
                    );
                    if (line == null) {
                        break;
                    }
                    this.on_log_line (line);
                }
            } catch (GLib.Error e) {
                // 读取取消或流关闭
            }
        }

        private void on_log_line (string line) {
            this.emit_log (line);
            string lower = line.down ();
            if ("connected to server" in lower || "c : connected" in lower || "tunnel ready" in lower || (lower.has_prefix ("connected") && !("not connected" in lower))) {
                if (this.state == TunnelState.CONNECTING) {
                    if (this.connect_timeout_id != 0) {
                        GLib.Source.remove (this.connect_timeout_id);
                        this.connect_timeout_id = 0;
                    }
                    this.reconnect_attempt = 0;
                    this.change_state (TunnelState.CONNECTED);

                    // 成功连上后，向 nftables 插入 cgroup 过滤规则：只有勾选的软件走代理，其余全部直连
                    var p = this.active_profile;
                    bool ipv6 = (p != null) ? p.ipv6 : false;
                    this.nft_manager.apply_cgroup_filter (this.local_proxy_port, ipv6);
                    this.sync_process_monitor_targets ();
                    this.process_monitor.start ();
                    this.emit_log ("Per-app proxy active: only checked applications are routed through proxy.");
                }
            }
        }

        public void disconnect_tunnel () {
            if (this.state == TunnelState.DISCONNECTED || this.state == TunnelState.DISCONNECTING) {
                return;
            }

            // 用户手动断开，重置重试计数并取消重连
            this.cancel_reconnect ();
            this.reconnect_attempt = 0;

            if (this.connect_timeout_id != 0) {
                GLib.Source.remove (this.connect_timeout_id);
                this.connect_timeout_id = 0;
            }

            if (this.cancellable != null) {
                this.cancellable.cancel ();
            }

            this.change_state (TunnelState.DISCONNECTING);
            this.emit_log ("Disconnecting tunnel...");

            // 彻底清理运行时防火墙规则与 cgroup
            this.cleanup_proxy_runtime ();

            if (this.process != null) {
                this.process.send_signal (Posix.Signal.INT);
                GLib.Timeout.add_seconds (3, () => {
                    if (this.state == TunnelState.DISCONNECTING && this.process != null) {
                        this.process.force_exit ();
                    }
                    return false;
                });
            } else {
                this.change_state (TunnelState.DISCONNECTED);
            }
        }

        private void on_process_exit () {
            if (this.connect_timeout_id != 0) {
                GLib.Source.remove (this.connect_timeout_id);
                this.connect_timeout_id = 0;
            }

            int exit_status = 0;
            if (this.process != null) {
                exit_status = this.process.get_exit_status ();
                this.emit_log (@"Process exited with status $(exit_status)");
            }

            this.process = null;

            if (this.state == TunnelState.DISCONNECTING) {
                this.cleanup_proxy_runtime ();
                this.change_state (TunnelState.DISCONNECTED);
                this.reconnect_attempt = 0;
            } else if (this.state != TunnelState.DISCONNECTED) {
                // 异常掉线：清理当前规则后触发无限自动重连
                this.cleanup_proxy_runtime ();
                this.change_state (TunnelState.ERROR);
                this.schedule_auto_reconnect ();
            }
        }

        private void schedule_auto_reconnect () {
            if (this.reconnect_timeout_id != 0) {
                return;
            }

            this.reconnect_attempt++;
            // 指数退避：2s, 4s, 8s, 16s, 30s, 30s...
            uint delay = (uint) int.min (
                (int) (BASE_RECONNECT_DELAY * (1 << int.min (this.reconnect_attempt - 1, 4))),
                (int) MAX_RECONNECT_DELAY
            );

            this.emit_log (@"Connection dropped. Auto-reconnecting in $(delay)s (attempt #$(this.reconnect_attempt), infinite retry)...");

            this.reconnect_timeout_id = GLib.Timeout.add_seconds (delay, () => {
                this.reconnect_timeout_id = 0;
                if (this.state == TunnelState.ERROR || this.state == TunnelState.DISCONNECTED) {
                    this.emit_log (@"Auto-reconnecting now (attempt #$(this.reconnect_attempt))...");
                    this.connect_active ();
                }
                return false;
            });
        }

        public void cancel_reconnect () {
            if (this.reconnect_timeout_id != 0) {
                GLib.Source.remove (this.reconnect_timeout_id);
                this.reconnect_timeout_id = 0;
            }
        }

        public void cleanup_proxy_runtime () {
            this.process_monitor.stop ();
            this.nft_manager.cleanup_all_sshuttle_tables (this.local_proxy_port);
            this.cgroup_manager.cleanup_and_destroy ();
        }

        private void on_app_rules_changed () {
            this.sync_process_monitor_targets ();
            if (this.state == TunnelState.CONNECTED) {
                var p = this.active_profile;
                bool ipv6 = (p != null) ? p.ipv6 : false;
                this.nft_manager.apply_cgroup_filter (this.local_proxy_port, ipv6);
                this.process_monitor.start ();
            }
        }

        private void sync_process_monitor_targets () {
            string[] proxy_app_ids = this.config_manager.get_proxy_apps ();
            var apps = AppScanner.scan_apps ();
            var target_execs = new GLib.GenericArray<string> ();

            for (uint i = 0; i < apps.length; i++) {
                var app = apps[i];
                for (uint j = 0; j < proxy_app_ids.length; j++) {
                    if (proxy_app_ids[j] == app.id || proxy_app_ids[j] == app.exec_name) {
                        target_execs.add (app.exec_name);
                        break;
                    }
                }
            }

            var arr = new string[target_execs.length];
            for (uint i = 0; i < target_execs.length; i++) {
                arr[i] = target_execs[i];
            }
            this.process_monitor.set_targets (arr);
            this.emit_log (@"Proxied apps updated: $(target_execs.length) app(s) checked for proxy.");
        }
    }
}

namespace Sshuttle {

    public class AppLogEntry : Object {
        public string timestamp { get; set; }
        public string app_name { get; set; }
        public string message { get; set; }

        public AppLogEntry (string timestamp, string app_name, string message) {
            this.timestamp = timestamp;
            this.app_name = app_name;
            this.message = message;
        }
    }

    public class SocketTrafficEntry : Object {
        public uint64 sent;
        public uint64 rcv;

        public SocketTrafficEntry (uint64 sent, uint64 rcv) {
            this.sent = sent;
            this.rcv = rcv;
        }
    }

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
        public DnsProxy dns_proxy { get; private set; }
        public int local_proxy_port { get; set; default = 12300; }

        public int reconnect_attempt { get; private set; default = 0; }
        private uint reconnect_timeout_id = 0;
        private const uint BASE_RECONNECT_DELAY = 2;
        private const uint MAX_RECONNECT_DELAY = 30;

        private GLib.Subprocess? process = null;
        private GLib.Cancellable? cancellable = null;
        private uint connect_timeout_id = 0;
        private const uint MAX_LOGS = 1000;

        private uint speed_timer_id = 0;
        private uint64 prev_bytes_sent = 0;
        private uint64 prev_bytes_rcv = 0;
        public string current_up_speed { get; private set; default = "0.0 kb/s"; }
        public string current_down_speed { get; private set; default = "0.0 kb/s"; }

        private GLib.HashTable<string, SocketTrafficEntry> active_sockets;
        private GLib.HashTable<string, string> proc_to_app_id;
        private uint traffic_save_counter = 0;

        public signal void speed_updated (string up_speed, string down_speed);

        public GLib.GenericArray<AppLogEntry> app_logs { get; private set; }
        public GLib.GenericArray<string> proxy_logs { get; private set; }

        public signal void app_log_received (AppLogEntry entry);
        public signal void proxy_log_received (string line);

        public Profile? active_profile {
            owned get {
                return this.config_manager.get_active_profile ();
            }
        }

        public TunnelManager (ConfigManager config_manager) {
            this.config_manager = config_manager;
            this.log_history = new GLib.GenericArray<string> ();
            this.app_logs = new GLib.GenericArray<AppLogEntry> ();
            this.proxy_logs = new GLib.GenericArray<string> ();

            this.active_sockets = new GLib.HashTable<string, SocketTrafficEntry> (GLib.str_hash, GLib.str_equal);
            this.proc_to_app_id = new GLib.HashTable<string, string> (GLib.str_hash, GLib.str_equal);

            this.cgroup_manager = new CgroupManager ();
            this.nft_manager = new NftManager ();
            this.process_monitor = new ProcessMonitor (this.cgroup_manager);
            this.dns_proxy = new DnsProxy (this.config_manager, this.nft_manager);

            // 启动时主动清除任何可能的上次异常残留（确保纯运行时无残余）
            this.cleanup_proxy_runtime ();

            this.config_manager.app_rules_changed.connect (this.on_app_rules_changed);
            this.config_manager.domain_rules_changed.connect (this.on_domain_rules_changed);
            this.config_manager.blacklist_changed.connect (this.on_blacklist_changed);
            this.process_monitor.process_migrated.connect (this.on_process_migrated);
            this.dns_proxy.dns_resolved.connect (this.on_dns_resolved);
        }

        private void on_dns_resolved (string domain, string action, string[] ips) {
            string ip_str = (ips.length > 0) ? string.joinv (", ", ips) : "no IP";
            this.emit_app_log ("DNS", @"$(domain) -> $(action) [$(ip_str)]");
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

        public void emit_app_log (string app_name, string message) {
            GLib.Idle.add (() => {
                var now = new GLib.DateTime.now_local ();
                string time_str = now.format ("%H:%M:%S");
                var entry = new AppLogEntry (time_str, app_name, message);
                this.app_logs.add (entry);
                if (this.app_logs.length > MAX_LOGS) {
                    this.app_logs.remove_index (0);
                }
                this.app_log_received (entry);

                string text = @"[$time_str] [$app_name] $message";
                this.log_history.add (text);
                if (this.log_history.length > MAX_LOGS) {
                    this.log_history.remove_index (0);
                }
                this.log_received (text);
                print ("%s\n", text);
                return GLib.Source.REMOVE;
            });
        }

        public void emit_proxy_log (string line) {
            GLib.Idle.add (() => {
                this.proxy_logs.add (line);
                if (this.proxy_logs.length > MAX_LOGS) {
                    this.proxy_logs.remove_index (0);
                }
                this.proxy_log_received (line);

                this.log_history.add (line);
                if (this.log_history.length > MAX_LOGS) {
                    this.log_history.remove_index (0);
                }
                this.log_received (line);
                print ("%s\n", line);
                return GLib.Source.REMOVE;
            });
        }

        private void emit_log (string text) {
            this.emit_app_log ("System", text);
        }

        public void clear_logs () {
            this.log_history.remove_range (0, this.log_history.length);
            this.app_logs.remove_range (0, this.app_logs.length);
            this.proxy_logs.remove_range (0, this.proxy_logs.length);
        }

        public void clear_app_logs () {
            this.app_logs.remove_range (0, this.app_logs.length);
        }

        public void clear_proxy_logs () {
            this.proxy_logs.remove_range (0, this.proxy_logs.length);
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

        /**
         * 探测 127.0.0.1 上的可用端口（从 start_port 开始向上顺延查找），防止端口冲突
         */
        private int find_available_local_port (int start_port = 12300) {
            for (int port = start_port; port < start_port + 100; port++) {
                try {
                    var s = new GLib.Socket (GLib.SocketFamily.IPV4, GLib.SocketType.STREAM, GLib.SocketProtocol.TCP);
                    var addr = new GLib.InetSocketAddress (new GLib.InetAddress.from_string ("127.0.0.1"), (uint16) port);
                    s.bind (addr, false);
                    s.close ();
                    return port;
                } catch (GLib.Error e) {
                    // 当前端口已被占用，继续探测下一个端口
                }
            }
            return start_port;
        }

        public void start_tunnel (Profile profile) {
            if (this.state == TunnelState.CONNECTING || this.state == TunnelState.CONNECTED) {
                return;
            }

            // 取消正在等待的自动重连倒计时
            this.cancel_reconnect ();

            this.change_state (TunnelState.CONNECTING);

            try {
                // 清理可能残留的孤儿 sshuttle 进程
                try {
                    GLib.Process.spawn_command_line_sync ("pkill -9 -f 'sshuttle.*127.0.0.1'");
                } catch (GLib.Error e) {
                    // 忽略无匹配进程报错
                }

                // 探测空闲端口，避免与已有服务冲突
                this.local_proxy_port = this.find_available_local_port (12300);

                // 启动按软件代理监控与 DNS 分流器
                this.sync_process_monitor_targets ();
                this.process_monitor.start ();
                this.dns_proxy.start ();

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
            string lower = line.down ();

            // 过滤无意义的底层 DNS 调试日志
            if ("dns request from" in lower) {
                return;
            }

            this.emit_proxy_log (line);

            if ("dns listening on" in lower) {
                // 捕获 sshuttle 内部安全 DNS 端口 (例如 "c : DNS listening on ('127.0.0.1', 12299).")
                try {
                    var r = new GLib.Regex ("dns listening on \\([^,]+,\\s*(\\d+)\\)", GLib.RegexCompileFlags.CASELESS);
                    GLib.MatchInfo info;
                    if (r.match (line, 0, out info)) {
                        string port_str = info.fetch (1);
                        uint16 port = (uint16) int.parse (port_str);
                        if (port > 0) {
                            this.dns_proxy.remote_dns_port = port;
                            this.emit_log (@"Tunnel remote DNS ready on port $(port)");
                        }
                    }
                } catch (GLib.RegexError e) {
                }
            }

            if ("connected to server" in lower || "c : connected" in lower || "tunnel ready" in lower || (lower.has_prefix ("connected") && !("not connected" in lower))) {
                if (this.state == TunnelState.CONNECTING) {
                    if (this.connect_timeout_id != 0) {
                        GLib.Source.remove (this.connect_timeout_id);
                        this.connect_timeout_id = 0;
                    }
                    this.reconnect_attempt = 0;
                    this.change_state (TunnelState.CONNECTED);

                    // 成功连上后，向 nftables 插入 cgroup 过滤规则与黑名单规则
                    var p = this.active_profile;
                    bool ipv6 = (p != null) ? p.ipv6 : false;
                    this.dns_proxy.start ();
                    this.nft_manager.apply_cgroup_filter (this.local_proxy_port, ipv6, this.config_manager.get_domain_default_policy ());
                    this.nft_manager.apply_blacklist_filter ();
                    this.sync_process_monitor_targets ();
                    this.process_monitor.start ();
                    this.start_speed_monitor ();
                    this.emit_log ("Per-app proxy and blacklist active.");
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
            // 用户明确要求：重连需要迅速，1s内重连
            uint delay = 1;

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
            this.stop_speed_monitor ();
            this.dns_proxy.stop ();
            this.dns_proxy.remote_dns_port = 0;
            this.process_monitor.stop ();
            this.nft_manager.cleanup_all_sshuttle_tables (this.local_proxy_port);
            this.cgroup_manager.cleanup_and_destroy ();
            this.active_sockets.remove_all ();
            this.config_manager.save_settings ();
        }

        private void start_speed_monitor () {
            this.stop_speed_monitor ();
            this.prev_bytes_sent = 0;
            this.prev_bytes_rcv = 0;

            this.speed_timer_id = GLib.Timeout.add_seconds (1, () => {
                if (this.state != TunnelState.CONNECTED) {
                    this.speed_timer_id = 0;
                    return false;
                }

                this.update_traffic_speed ();
                return true;
            });
        }

        private void stop_speed_monitor () {
            if (this.speed_timer_id != 0) {
                GLib.Source.remove (this.speed_timer_id);
                this.speed_timer_id = 0;
            }
            this.prev_bytes_sent = 0;
            this.prev_bytes_rcv = 0;
            this.current_up_speed = "0.0 kb/s";
            this.current_down_speed = "0.0 kb/s";
            this.speed_updated (this.current_up_speed, this.current_down_speed);
        }

        private void update_traffic_speed () {
            string output = "";
            bool is_peer = false;

            var p = this.active_profile;
            if (p != null && p.host != "") {
                try {
                    string[] argv = { "ss", "-tin", "dst", p.host };
                    int status;
                    string stdout_buf;
                    GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_buf, null, out status);
                    if ("bytes_sent:" in stdout_buf || "bytes_received:" in stdout_buf) {
                        output = stdout_buf;
                        is_peer = true;
                    }
                } catch (GLib.Error e) {
                }
            }

            if (output == "") {
                try {
                    string[] argv = { "ss", "-tin", "sport", "=", this.local_proxy_port.to_string () };
                    int status;
                    string stdout_buf;
                    GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_buf, null, out status);
                    output = stdout_buf;
                    is_peer = false;
                } catch (GLib.Error e) {
                }
            }

            if (output == "") {
                return;
            }

            uint64 total_sent = 0;
            uint64 total_rcv = 0;

            try {
                var regex_sent = new GLib.Regex ("bytes_sent:(\\d+)");
                GLib.MatchInfo info_sent;
                if (regex_sent.match (output, 0, out info_sent)) {
                    while (info_sent.matches ()) {
                        string val = info_sent.fetch (1);
                        total_sent += uint64.parse (val);
                        info_sent.next ();
                    }
                }

                var regex_rcv = new GLib.Regex ("bytes_received:(\\d+)");
                GLib.MatchInfo info_rcv;
                if (regex_rcv.match (output, 0, out info_rcv)) {
                    while (info_rcv.matches ()) {
                        string val = info_rcv.fetch (1);
                        total_rcv += uint64.parse (val);
                        info_rcv.next ();
                    }
                }
            } catch (GLib.Error e) {
            }

            uint64 cur_up = 0;
            uint64 cur_down = 0;

            if (is_peer) {
                cur_up = total_sent;
                cur_down = total_rcv;
            } else {
                cur_up = total_rcv;
                cur_down = total_sent;
            }

            uint64 up_bytes_per_sec = 0;
            uint64 down_bytes_per_sec = 0;

            if (this.prev_bytes_sent > 0 && cur_up >= this.prev_bytes_sent) {
                up_bytes_per_sec = cur_up - this.prev_bytes_sent;
            }
            if (this.prev_bytes_rcv > 0 && cur_down >= this.prev_bytes_rcv) {
                down_bytes_per_sec = cur_down - this.prev_bytes_rcv;
            }

            this.prev_bytes_sent = cur_up;
            this.prev_bytes_rcv = cur_down;

            this.current_up_speed = format_speed (up_bytes_per_sec);
            this.current_down_speed = format_speed (down_bytes_per_sec);

            this.update_app_traffic_stats ();

            this.speed_updated (this.current_up_speed, this.current_down_speed);
        }

        private void update_app_traffic_stats () {
            string stdout_buf;
            try {
                string[] argv = { "ss", "-tipn", "--cgroup", "cgroup", "=", "sshuttle-proxy" };
                int status;
                GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_buf, null, out status);
            } catch (GLib.Error e) {
                return;
            }

            if (stdout_buf == null || stdout_buf == "") {
                return;
            }

            var seen_sockets = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
            string[] lines = stdout_buf.split ("\n");

            string current_sock_key = "";
            string current_app_id = "";

            try {
                var rx_user = new GLib.Regex ("users:\\(\\(\"([^\"]+)\"");
                var rx_sent = new GLib.Regex ("bytes_sent:(\\d+)");
                var rx_rcv = new GLib.Regex ("bytes_received:(\\d+)");

                for (int i = 0; i < lines.length; i++) {
                    string line = lines[i].strip ();
                    if (line == "") {
                        continue;
                    }

                    if (!line.has_prefix ("cubic") && !line.has_prefix ("bbr") && !line.has_prefix ("reno") && ("cgroup:" in line || "users:" in line)) {
                        string[] tokens = line.split (" ");
                        var non_empty = new GLib.GenericArray<string> ();
                        foreach (var t in tokens) {
                            if (t != "") {
                                non_empty.add (t);
                            }
                        }

                        if (non_empty.length >= 5) {
                            string local_addr = non_empty[3];
                            string peer_addr = non_empty[4];
                            current_sock_key = @"$(local_addr)->$(peer_addr)";
                            seen_sockets.insert (current_sock_key, true);

                            string proc_name = "";
                            GLib.MatchInfo info_user;
                            if (rx_user.match (line, 0, out info_user)) {
                                proc_name = info_user.fetch (1).strip ().down ();
                            }

                            current_app_id = this.resolve_app_id_for_process (proc_name);
                        }
                    } else if (current_sock_key != "" && current_app_id != "") {
                        uint64 cur_sent = 0;
                        uint64 cur_rcv = 0;

                        GLib.MatchInfo info_sent;
                        if (rx_sent.match (line, 0, out info_sent)) {
                            cur_sent = uint64.parse (info_sent.fetch (1));
                        }

                        GLib.MatchInfo info_rcv;
                        if (rx_rcv.match (line, 0, out info_rcv)) {
                            cur_rcv = uint64.parse (info_rcv.fetch (1));
                        }

                        if (cur_sent > 0 || cur_rcv > 0) {
                            var prev = this.active_sockets.lookup (current_sock_key);
                            uint64 delta_up = 0;
                            uint64 delta_down = 0;

                            if (prev != null) {
                                if (cur_sent >= prev.sent) {
                                    delta_up = cur_sent - prev.sent;
                                }
                                if (cur_rcv >= prev.rcv) {
                                    delta_down = cur_rcv - prev.rcv;
                                }
                                prev.sent = cur_sent;
                                prev.rcv = cur_rcv;
                            } else {
                                delta_up = cur_sent;
                                delta_down = cur_rcv;
                                this.active_sockets.insert (current_sock_key, new SocketTrafficEntry (cur_sent, cur_rcv));
                            }

                            if (delta_up > 0 || delta_down > 0) {
                                this.config_manager.add_app_traffic (current_app_id, delta_up, delta_down);
                            }
                        }

                        current_sock_key = "";
                        current_app_id = "";
                    }
                }
            } catch (GLib.RegexError e) {
            }

            var closed_keys = new GLib.GenericArray<string> ();
            this.active_sockets.foreach ((k, v) => {
                if (!seen_sockets.contains (k)) {
                    closed_keys.add (k);
                }
            });
            for (uint i = 0; i < closed_keys.length; i++) {
                this.active_sockets.remove (closed_keys[i]);
            }

            this.traffic_save_counter++;
            if (this.traffic_save_counter >= 10) {
                this.traffic_save_counter = 0;
                this.config_manager.save_settings ();
            }
        }

        private string resolve_app_id_for_process (string proc_name) {
            string p = proc_name.strip ().down ();
            if (p == "") {
                return "other";
            }

            string? mapped = this.proc_to_app_id.lookup (p);
            if (mapped != null && mapped != "") {
                return mapped;
            }

            var apps = AppScanner.scan_apps ();
            for (uint i = 0; i < apps.length; i++) {
                var app = apps[i];
                if (app.exec_name.down () == p || app.id.replace (".desktop", "").down () == p) {
                    this.proc_to_app_id.insert (p, app.id);
                    return app.id;
                }
            }

            return p;
        }

        public static string format_bytes (uint64 bytes) {
            double b = (double) bytes;
            if (b < 1024.0) {
                return @"$(bytes) B";
            } else if (b < 1024.0 * 1024.0) {
                double kb = b / 1024.0;
                return "%.1f KB".printf (kb);
            } else if (b < 1024.0 * 1024.0 * 1024.0) {
                double mb = b / (1024.0 * 1024.0);
                return "%.2f MB".printf (mb);
            } else {
                double gb = b / (1024.0 * 1024.0 * 1024.0);
                return "%.2f GB".printf (gb);
            }
        }

        public static string format_speed (uint64 bytes_per_sec) {
            double b = (double) bytes_per_sec;
            if (b < 1024.0 * 1024.0) {
                double kb = b / 1024.0;
                return "%.1f kb/s".printf (kb);
            } else {
                double mb = b / (1024.0 * 1024.0);
                return "%.2f mb/s".printf (mb);
            }
        }

        private void on_app_rules_changed () {
            this.sync_process_monitor_targets ();
            if (this.state == TunnelState.CONNECTED) {
                var p = this.active_profile;
                bool ipv6 = (p != null) ? p.ipv6 : false;
                this.nft_manager.apply_cgroup_filter (this.local_proxy_port, ipv6, this.config_manager.get_domain_default_policy ());
                this.process_monitor.start ();
            }
        }

        private void on_domain_rules_changed () {
            if (this.state == TunnelState.CONNECTED) {
                var p = this.active_profile;
                bool ipv6 = (p != null) ? p.ipv6 : false;
                this.nft_manager.apply_cgroup_filter (this.local_proxy_port, ipv6, this.config_manager.get_domain_default_policy ());
            }
        }

        private void on_blacklist_changed () {
            this.sync_process_monitor_targets ();
            if (this.state == TunnelState.CONNECTED) {
                this.nft_manager.apply_blacklist_filter ();
                this.process_monitor.start ();
            }
        }

        private void on_process_migrated (string app_name, int pid, string cgroup_type) {
            if (cgroup_type == "block") {
                this.emit_app_log (app_name, @"Blocked from network (PID: $(pid))");
            } else {
                this.emit_app_log (app_name, @"Routed through proxy (PID: $(pid))");
            }
        }

        private void sync_process_monitor_targets () {
            string[] proxy_app_ids = this.config_manager.get_proxy_apps ();
            string[] block_app_ids = this.config_manager.get_blocked_apps ();
            string[] block_procs = this.config_manager.get_blocked_processes ();
            var apps = AppScanner.scan_apps ();

            var target_execs = new GLib.GenericArray<string> ();
            var block_execs = new GLib.GenericArray<string> ();

            // App rules for proxy
            for (uint i = 0; i < apps.length; i++) {
                var app = apps[i];
                for (uint j = 0; j < proxy_app_ids.length; j++) {
                    if (proxy_app_ids[j] == app.id || proxy_app_ids[j] == app.exec_name) {
                        target_execs.add (app.exec_name);
                        this.proc_to_app_id.insert (app.exec_name.down (), app.id);
                        string app_id_clean = app.id.replace (".desktop", "").down ();
                        if (app_id_clean != "" && app_id_clean != app.exec_name) {
                            target_execs.add (app_id_clean);
                            this.proc_to_app_id.insert (app_id_clean, app.id);
                        }

                        // 常见应用与浏览器二进制及 Flatpak 进程名别名补充
                        if (app.exec_name == "google-chrome-stable" || app.exec_name == "google-chrome" || "chrome" in app.id.down ()) {
                            target_execs.add ("chrome");
                            this.proc_to_app_id.insert ("chrome", app.id);
                            this.proc_to_app_id.insert ("google-chrome", app.id);
                            this.proc_to_app_id.insert ("google-chrome-stable", app.id);
                        } else if (app.exec_name == "firefox" || "firefox" in app.id.down ()) {
                            target_execs.add ("firefox-bin");
                            this.proc_to_app_id.insert ("firefox", app.id);
                            this.proc_to_app_id.insert ("firefox-bin", app.id);
                        } else if (app.exec_name == "telegram" || "telegram" in app.id.down () || "telegram" in app.exec_name.down ()) {
                            target_execs.add ("telegram");
                            target_execs.add ("telegram-desktop");
                            target_execs.add ("telegramdesktop");
                            target_execs.add ("org.telegram.desktop");
                            this.proc_to_app_id.insert ("telegram", app.id);
                            this.proc_to_app_id.insert ("telegram-desktop", app.id);
                            this.proc_to_app_id.insert ("telegramdesktop", app.id);
                            this.proc_to_app_id.insert ("org.telegram.desktop", app.id);
                        }
                        break;
                    }
                }
            }

            // App rules for blacklist
            for (uint i = 0; i < apps.length; i++) {
                var app = apps[i];
                for (uint j = 0; j < block_app_ids.length; j++) {
                    if (block_app_ids[j] == app.id || block_app_ids[j] == app.exec_name) {
                        block_execs.add (app.exec_name);
                        string app_id_clean = app.id.replace (".desktop", "").down ();
                        if (app_id_clean != "" && app_id_clean != app.exec_name) {
                            block_execs.add (app_id_clean);
                        }
                        break;
                    }
                }
            }

            // Process rules for blacklist
            for (uint i = 0; i < block_procs.length; i++) {
                string proc = block_procs[i].strip ();
                if (proc != "") {
                    block_execs.add (proc);
                }
            }

            var arr_targets = new string[target_execs.length];
            for (uint i = 0; i < target_execs.length; i++) {
                arr_targets[i] = target_execs[i];
            }
            this.process_monitor.set_targets (arr_targets);

            var arr_blocks = new string[block_execs.length];
            for (uint i = 0; i < block_execs.length; i++) {
                arr_blocks[i] = block_execs[i];
            }
            this.process_monitor.set_block_targets (arr_blocks);

            this.emit_app_log ("System", @"Rules updated: $(target_execs.length) proxied app(s), $(block_execs.length) blacklisted target(s).");
        }
    }
}

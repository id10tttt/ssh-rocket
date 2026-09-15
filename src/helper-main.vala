#if !RUNTIME_TESTS
[CCode (cheader_filename = "grp.h", cname = "initgroups")]
extern int init_groups (string user, Posix.gid_t group);
[CCode (cheader_filename = "sys/file.h", cname = "flock")]
extern int lock_file (int fd, int operation);
#endif

namespace Sshuttle {
    /** 特权操作只通过已验证用户的私有 D-Bus 连接开放。 */
    public class RuntimeService : Object, Runtime {
        private CgroupManager groups = new CgroupManager ();
        private NftManager nft = new NftManager ();
        private uint uid;
        private GLib.Subprocess? tunnel;
        private GLib.Subprocess? forwarder;
        private TunRouter router = new TunRouter ();
        private bool forwarder_ready;
        private int port = 0;
        private bool prepared = false;
        private bool stopping = false;
        private bool closing = false;
        private uint kill_timeout = 0;
        public signal void finished ();

        public RuntimeService (uint uid) {
            this.uid = uid;
        }

        public bool prepare () throws GLib.Error {
            if (prepared) {
                return true;
            }
            if (nft.has_active_sshuttle_tables ()) {
                throw new GLib.IOError.BUSY ("Another SSH Rocket or sshuttle session is active");
            }
            if (groups.is_runtime_cgroup_created () || groups.is_cgroup_created () || groups.is_block_cgroup_created ()) {
                throw new GLib.IOError.BUSY ("SSH Rocket cgroups already exist; finish the previous session first");
            }
            if (!groups.ensure_runtime_cgroup () || !groups.ensure_proxy_cgroup () || !groups.ensure_block_cgroup ()) {
                groups.cleanup_and_destroy ();
                throw new GLib.IOError.FAILED ("Failed to prepare cgroup v2 routing directories");
            }
            prepared = true;
            return true;
        }

        public bool cgroup (string operation, int pid) throws GLib.Error {
            if (!prepared) {
                return false;
            }
            if (operation == "ensure-proxy" || operation == "ensure-block") {
                return groups.is_cgroup_created () && groups.is_block_cgroup_created ();
            }
            Posix.Stat process_stat = Posix.Stat ();
            if (pid <= 1 || Posix.stat (@"/proc/$(pid)", out process_stat) != 0 || process_stat.st_uid != uid) {
                return false;
            }
            string process_group;
            if (GLib.FileUtils.get_contents (@"/proc/$(pid)/cgroup", out process_group) &&
                "sshrocket-runtime" in process_group) return false;
            switch (operation) {
                case "proxy": return groups.move_pid_to_proxy (pid);
                case "block": return groups.move_pid_to_block (pid);
                case "default": return groups.move_pid_to_default (pid);
                default: throw new GLib.IOError.INVALID_ARGUMENT ("Invalid cgroup operation");
            }
        }

        public bool has_active_tables () throws GLib.Error {
            return nft.has_active_sshuttle_tables ();
        }

        private void require_port (int requested) throws GLib.Error {
            if (tunnel == null || stopping || port != requested) {
                throw new GLib.IOError.CLOSED ("No matching active tunnel");
            }
        }

        public bool base_chains_exist (int port, bool ipv6) throws GLib.Error {
            require_port (port);
            return nft.base_chains_exist (port, ipv6);
        }

        public bool apply_routing (int port, bool ipv6, string policy, string[] networks,
            string[] patterns, string[] actions) throws GLib.Error {
            require_port (port);
            if (patterns.length != actions.length || (policy != "proxy" && policy != "direct")) {
                throw new GLib.IOError.INVALID_ARGUMENT ("Invalid routing configuration");
            }
            DomainRule[] rules = new DomainRule[patterns.length];
            for (int i = 0; i < patterns.length; i++) {
                rules[i] = new DomainRule (patterns[i], actions[i]);
            }
            return nft.apply_cgroup_filter (port, ipv6, policy, networks, rules);
        }

        public bool apply_blacklist () throws GLib.Error {
            require_port (port);
            return nft.apply_blacklist_filter ();
        }

        public void add_ips (string[] ips, string action) throws GLib.Error {
            require_port (port);
            nft.add_ips_to_set (ips, action);
        }

        public void flush_ips () throws GLib.Error {
            require_port (port);
            nft.flush_ip_sets ();
        }

        public void cleanup () throws GLib.Error {
            // OpenSSH 退出后停止 tun2socks，再释放路由、虚拟网卡和 cgroup。
            if (tunnel != null) {
                stop_tunnel ();
                return;
            }
            cleanup_owned_runtime ();
        }

        private void cleanup_owned_runtime () {
            if (!prepared) {
                return;
            }
            nft.cleanup_all_sshuttle_tables (port);
            router.stop ();
            groups.cleanup_and_destroy ();
            port = 0;
            prepared = false;
        }

        /** 路由参数严格校验；客户端 SSH 命令只允许在降权后执行。 */
        public void start_tunnel (string[] argv, string password, string agent) throws GLib.Error {
            if (closing || tunnel != null || !prepared || argv.length < 2 || argv[0] != "ssh-rocket") {
                throw new GLib.IOError.BUSY ("Tunnel runtime is not ready");
            }
            string ssh_command = "";
            string remote = "";
            int requested_port = 0;
            bool ipv6 = true;
            string[] routes = {};
            for (int i = 1; i < argv.length; i++) {
                string arg = argv[i];
                if (arg == "-e" || arg == "-r" || arg == "-l" || arg == "-x") {
                    if (++i >= argv.length) throw new GLib.IOError.INVALID_ARGUMENT ("Missing tunnel argument");
                    string value = argv[i];
                    if (arg == "-e") {
                        if (ssh_command != "" || value == "") throw new GLib.IOError.INVALID_ARGUMENT ("Duplicate SSH command");
                        ssh_command = value;
                    } else if (arg == "-r") {
                        if (remote != "" || value == "" || value.has_prefix ("-"))
                            throw new GLib.IOError.INVALID_ARGUMENT ("Invalid SSH destination");
                        remote = value;
                    } else if (arg == "-l") {
                        var match = new GLib.Regex ("^127\\.0\\.0\\.1:([0-9]+)(,\\[::1\\]:([0-9]+))?$");
                        GLib.MatchInfo info;
                        if (!match.match (value, 0, out info) || requested_port != 0 ||
                            !int.try_parse (info.fetch (1), out requested_port) || requested_port < 1024 || requested_port > 65534 ||
                            (info.fetch (3) != "" && info.fetch (3) != info.fetch (1)))
                            throw new GLib.IOError.INVALID_ARGUMENT ("Invalid loopback listen address");
                    }
                    // 排除网络由 apply_routing 统一规范化，不拼接客户端字符串到特权命令。
                } else if (arg == "--dns") {
                    // DNS 本地端口转发已包含在经验证的 SSH 命令中。
                } else if (arg == "--disable-ipv6") {
                    ipv6 = false;
                } else {
                    string[] parts = arg.split ("/", 2);
                    var ip = new GLib.InetAddress.from_string (parts[0]);
                    int prefix = 0;
                    if (ip == null || (parts.length == 2 && (!int.try_parse (parts[1], out prefix) || prefix < 0 ||
                        prefix > (ip.get_family () == GLib.SocketFamily.IPV6 ? 128 : 32))))
                        throw new GLib.IOError.INVALID_ARGUMENT ("Invalid route");
                    routes += arg;
                }
            }
            if (ssh_command == "" || remote == "" || requested_port == 0 || routes.length == 0)
                throw new GLib.IOError.INVALID_ARGUMENT ("Incomplete tunnel arguments");
            port = requested_port;
            stopping = false;
            forwarder_ready = false;
            try {
                router.start (uid, ipv6);
                if (!nft.create_base_chains (port, ipv6, routes))
                    throw new GLib.IOError.FAILED ("Failed to create SSH Rocket routing chains");
                var launcher = new GLib.SubprocessLauncher (
                    GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE);
                launcher.set_environ ({ "PATH=/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "LANG=C.UTF-8", "HOME=/root" });
                launcher.set_cwd ("/");
                string tun_command = "%s --device tun://%s --proxy socks5://127.0.0.1:%d --loglevel info".printf (
                    GLib.Shell.quote (Config.TUN2SOCKS_PATH), TunRouter.DEVICE, port);
                forwarder = Native.spawnv (launcher, { Config.HELPER_PATH, "--user-ssh", uid.to_string (), tun_command });
                launcher.setenv ("SSHPASS", password, true);
                launcher.setenv ("SSH_AUTH_SOCK", agent, true);
                tunnel = Native.spawnv (launcher, { Config.HELPER_PATH, "--user-ssh", uid.to_string (),
                    ssh_command + " -- " + GLib.Shell.quote (remote) });
                read_log.begin (forwarder, forwarder.get_stdout_pipe ());
                read_log.begin (forwarder, forwarder.get_stderr_pipe ());
                watch_forwarder.begin (forwarder);
                read_log.begin (tunnel, tunnel.get_stdout_pipe ());
                read_log.begin (tunnel, tunnel.get_stderr_pipe ());
                wait_tunnel.begin (tunnel);
                probe_ready.begin (tunnel);
            } catch (GLib.Error e) {
                if (forwarder != null) {
                    forwarder.force_exit ();
                    forwarder.wait (null);
                    forwarder = null;
                }
                cleanup_owned_runtime ();
                throw e;
            }
        }

        /** SOCKS 握手成功且 TUN 协议栈就绪后，才通知界面安装分流规则。 */
        private async void probe_ready (GLib.Subprocess child) {
            while (tunnel == child && !stopping) {
                try {
                    var client = new GLib.SocketClient ();
                    client.timeout = 1;
                    var connection = yield client.connect_to_host_async ("127.0.0.1", (uint16) port);
                    size_t count;
                    yield connection.output_stream.write_all_async ({ 5, 1, 0 }, GLib.Priority.DEFAULT, null, out count);
                    uint8[] reply = new uint8[2];
                    yield connection.input_stream.read_all_async (reply, GLib.Priority.DEFAULT, null, out count);
                    connection.close (null);
                    if (count == 2 && reply[0] == 5 && reply[1] == 0 && forwarder_ready && tunnel == child && !stopping) {
                        log_line ("SSH Rocket tunnel ready");
                        return;
                    }
                } catch (GLib.Error e) {}
                GLib.Timeout.add (100, () => { probe_ready.callback (); return false; });
                yield;
            }
        }

        private async void watch_forwarder (GLib.Subprocess child) {
            try {
                yield child.wait_async (null);
                if (forwarder == child && !stopping && tunnel != null) {
                    log_line ("tun2socks exited; stopping SSH tunnel");
                    stop_tunnel ();
                }
            } catch (GLib.Error e) { warning ("tun2socks wait: %s", e.message); }
        }

        private async void read_log (GLib.Subprocess child, GLib.InputStream stream) {
            var input = new GLib.DataInputStream (stream);
            try {
                string? line;
                while ((line = yield input.read_line_async (GLib.Priority.DEFAULT)) != null) {
                    if (forwarder == child && "[STACK]" in line) forwarder_ready = true;
                    if (tunnel == child || forwarder == child) {
                        log_line (line.make_valid ());
                    }
                }
            } catch (GLib.Error e) {
                warning ("Tunnel log stream failed: %s", e.message);
            }
        }

        private async void wait_tunnel (GLib.Subprocess child) {
            try {
                yield child.wait_async (null);
                int status = child.get_if_exited () ? child.get_exit_status () : -1;
                int signal_number = child.get_if_signaled () ? child.get_term_sig () : 0;
                if (kill_timeout != 0) {
                    GLib.Source.remove (kill_timeout);
                    kill_timeout = 0;
                }
                stopping = true;
                if (forwarder != null) {
                    forwarder.force_exit ();
                    yield forwarder.wait_async (null);
                    forwarder = null;
                }
                tunnel = null;
                cleanup_owned_runtime ();
                tunnel_exited (status, signal_number);
                if (closing) {
                    finished ();
                }
            } catch (GLib.Error e) {
                warning ("Tunnel wait failed: %s", e.message);
            }
        }

        public void stop_tunnel () throws GLib.Error {
            if (tunnel == null || stopping) {
                return;
            }
            stopping = true;
            tunnel.send_signal (Posix.Signal.INT);
            kill_timeout = GLib.Timeout.add_seconds (3, () => {
                kill_timeout = 0;
                if (tunnel != null) {
                    tunnel.force_exit ();
                }
                return GLib.Source.REMOVE;
            });
        }

        public void close_session () {
            closing = true;
            try {
                cleanup ();
            } catch (GLib.Error e) {
                warning ("Runtime cleanup failed: %s", e.message);
            }
            if (tunnel == null) {
                finished ();
            }
        }
    }
}

#if !RUNTIME_TESTS
// 密钥、ssh_config、known_hosts 以及远程命令始终在普通用户身份下访问。
int run_user_ssh (string[] args) {
    uint uid = 0;
    if (args.length < 4 || !uint.try_parse (args[2], out uid) || uid == 0) {
        return 2;
    }
    unowned Posix.Passwd? user = Posix.getpwuid (uid);
    if (user == null) {
        return 1;
    }
    string user_name = user.pw_name;
    string home_dir = user.pw_dir;
    Posix.gid_t group_id = user.pw_gid;
    try {
        var group_file = GLib.File.new_for_path ("/sys/fs/cgroup/sshrocket-runtime/cgroup.procs");
        var output = group_file.append_to (GLib.FileCreateFlags.NONE);
        output.write (((int) Posix.getpid ()).to_string ().data);
        output.close ();
    } catch (GLib.Error e) {
        stderr.printf ("Cannot isolate proxy transport: %s\n", e.message);
        return 1;
    }
    if (init_groups (user_name, group_id) != 0 || Posix.setgid (group_id) != 0 || Posix.setuid (uid) != 0) {
        return 1;
    }
    GLib.Environment.set_variable ("HOME", home_dir, true);
    GLib.Environment.set_variable ("USER", user_name, true);
    GLib.Environment.set_variable ("LOGNAME", user_name, true);
    Posix.chdir (home_dir);
    try {
        string[] command;
        GLib.Shell.parse_argv (args[3], out command);
        for (int i = 4; i < args.length; i++) {
            command += args[i];
        }
        Posix.execvp (command[0], command);
    } catch (GLib.Error e) {
        stderr.printf ("SSH command failed: %s\n", e.message);
    }
    return 1;
}

async void watch_parent (Sshuttle.RuntimeService service) {
    try {
        uint8[] buffer = new uint8[1];
        var input = new GLib.UnixInputStream (0, false);
        while ((yield input.read_async (buffer, GLib.Priority.DEFAULT)) > 0) {}
    } catch (GLib.Error e) {}
    service.close_session ();
}

int main (string[] args) {
    if (Posix.geteuid () != 0) {
        stderr.printf ("The runtime helper requires administrator authorization.\n");
        return 1;
    }
    if (args.length > 1 && args[1] == "--user-ssh") {
        return run_user_ssh (args);
    }
    uint uid = 0;
    string? caller = GLib.Environment.get_variable ("PKEXEC_UID");
    if (args.length != 1 || caller == null || !uint.try_parse (caller, out uid) || uid == 0) {
        return 2;
    }
    int lock_fd = Posix.open ("/run/sshuttle-gui.lock", Posix.O_CREAT | Posix.O_RDWR | Posix.O_NOFOLLOW | Posix.O_CLOEXEC, 0600);
    if (lock_fd < 0 || lock_file (lock_fd, 2 | 4) != 0) {
        stderr.printf ("Another SSH Rocket runtime helper is active.\n");
        return 1;
    }
    var loop = new GLib.MainLoop ();
    var service = new Sshuttle.RuntimeService (uid);
    service.finished.connect (() => { loop.quit (); });
    GLib.DBusConnection? active_connection = null;
    try {
        var observer = new GLib.DBusAuthObserver ();
        observer.authorize_authenticated_peer.connect ((stream, credentials) => {
            try {
                return credentials != null && credentials.get_unix_user () == uid;
            } catch (GLib.Error e) {
                return false;
            }
        });
        var server = new GLib.DBusServer.sync ("unix:abstract=sshuttle-gui-" + GLib.Uuid.string_random (), GLib.DBusServerFlags.NONE,
            GLib.DBus.generate_guid (), observer);
        server.new_connection.connect ((connection) => {
            if (active_connection != null) {
                return false;
            }
            try {
                connection.register_object<Sshuttle.Runtime> ("/io/github/idi0t/SshuttleGUI/Runtime", service);
                active_connection = connection;
                connection.set_exit_on_close (false);
                connection.on_closed.connect (() => { service.close_session (); });
                return true;
            } catch (GLib.Error e) {
                return false;
            }
        });
        server.start ();
        stdout.printf ("%s\n", server.get_client_address ());
        stdout.flush ();
        watch_parent.begin (service);
        GLib.Unix.signal_add (Posix.Signal.TERM, () => { service.close_session (); return false; });
        GLib.Unix.signal_add (Posix.Signal.INT, () => { service.close_session (); return false; });
        loop.run ();
        server.stop ();
    } catch (GLib.Error e) {
        stderr.printf ("Runtime helper failed: %s\n", e.message);
        return 1;
    }
    Posix.close (lock_fd);
    return 0;
}

#endif

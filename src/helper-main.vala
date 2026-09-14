[CCode (cheader_filename = "grp.h", cname = "initgroups")]
extern int init_groups (string user, Posix.gid_t group);
[CCode (cheader_filename = "sys/file.h", cname = "flock")]
extern int lock_file (int fd, int operation);

namespace Sshuttle {
    /** 特权操作只通过已验证用户的私有 D-Bus 连接开放。 */
    public class RuntimeService : Object, Runtime {
        private CgroupManager groups = new CgroupManager ();
        private NftManager nft = new NftManager ();
        private uint uid;
        private GLib.Subprocess? tunnel;
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
                throw new GLib.IOError.BUSY ("Another sshuttle session is active");
            }
            if (groups.is_cgroup_created () || groups.is_block_cgroup_created ()) {
                throw new GLib.IOError.BUSY ("SShuttle cgroups already exist; finish the previous session first");
            }
            if (!groups.ensure_proxy_cgroup () || !groups.ensure_block_cgroup ()) {
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
            // sshuttle 先退出并清理基础表，之后才释放本应用的规则与 cgroup。
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
            groups.cleanup_and_destroy ();
            port = 0;
            prepared = false;
        }

        /** 只接受客户端已有的参数集合，SSH 命令在降权后解析执行。 */
        public void start_tunnel (string[] argv, string password, string agent) throws GLib.Error {
            if (closing || tunnel != null || !prepared || argv.length < 2 || argv[0] != "sshuttle") {
                throw new GLib.IOError.BUSY ("Tunnel runtime is not ready");
            }
            string[] safe_args = { Config.SSHUTTLE_PATH };
            bool have_ssh = false;
            bool have_remote = false;
            bool have_listen = false;
            bool have_method = false;
            int requested_port = 0;
            for (int i = 1; i < argv.length; i++) {
                string arg = argv[i];
                if (arg == "-e" || arg == "-r" || arg == "-l" || arg == "-x" || arg == "--method") {
                    if (++i >= argv.length) {
                        throw new GLib.IOError.INVALID_ARGUMENT ("Missing tunnel argument");
                    }
                    string value = argv[i];
                    if (arg == "-e") {
                        if (have_ssh) {
                            throw new GLib.IOError.INVALID_ARGUMENT ("Duplicate SSH command");
                        }
                        have_ssh = true;
                        value = "%s --user-ssh %u %s".printf (
                            GLib.Shell.quote (Config.HELPER_PATH), uid, GLib.Shell.quote (value));
                    } else if (arg == "--method") {
                        if (have_method || value != "nft") {
                            throw new GLib.IOError.INVALID_ARGUMENT ("Only nft routing is supported");
                        }
                        have_method = true;
                    } else if (arg == "-l") {
                        if (have_listen) {
                            throw new GLib.IOError.INVALID_ARGUMENT ("Duplicate listen address");
                        }
                        have_listen = true;
                        var match = new GLib.Regex ("^127\\.0\\.0\\.1:([0-9]+)(,\\[::1\\]:([0-9]+))?$");
                        GLib.MatchInfo info;
                        if (!match.match (value, 0, out info) ||
                            !int.try_parse (info.fetch (1), out requested_port) || requested_port < 1024 || requested_port > 65535 ||
                            (info.fetch (3) != "" && info.fetch (3) != info.fetch (1))) {
                            throw new GLib.IOError.INVALID_ARGUMENT ("Invalid loopback listen address");
                        }
                    } else if (arg == "-r") {
                        if (have_remote || value == "" || value.has_prefix ("-")) {
                            throw new GLib.IOError.INVALID_ARGUMENT ("Invalid SSH destination");
                        }
                        have_remote = true;
                    }
                    safe_args += arg;
                    safe_args += value;
                } else if (arg == "--dns" || arg == "--disable-ipv6" || arg == "-v" || arg == "-vv") {
                    safe_args += arg;
                } else {
                    string[] parts = arg.split ("/", 2);
                    var ip = new GLib.InetAddress.from_string (parts[0]);
                    int prefix = 0;
                    if (ip == null || (parts.length == 2 && (!int.try_parse (parts[1], out prefix) || prefix < 0 ||
                        prefix > (ip.get_family () == GLib.SocketFamily.IPV6 ? 128 : 32)))) {
                        throw new GLib.IOError.INVALID_ARGUMENT ("Invalid route");
                    }
                    safe_args += arg;
                }
            }
            if (!have_ssh || !have_remote || !have_listen || !have_method) {
                throw new GLib.IOError.INVALID_ARGUMENT ("Incomplete tunnel arguments");
            }
            var launcher = new GLib.SubprocessLauncher (
                GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE);
            launcher.set_environ ({ "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "LANG=C.UTF-8", "HOME=/root" });
            launcher.setenv ("SSHPASS", password, true);
            launcher.setenv ("SSH_AUTH_SOCK", agent, true);
            launcher.set_cwd ("/");
            tunnel = launcher.spawnv (safe_args);
            port = requested_port;
            stopping = false;
            read_log.begin (tunnel, tunnel.get_stdout_pipe ());
            read_log.begin (tunnel, tunnel.get_stderr_pipe ());
            wait_tunnel.begin (tunnel);
        }

        private async void read_log (GLib.Subprocess child, GLib.InputStream stream) {
            var input = new GLib.DataInputStream (stream);
            try {
                string? line;
                while ((line = yield input.read_line_async (GLib.Priority.DEFAULT)) != null) {
                    if (tunnel == child) {
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
        stderr.printf ("Another SShuttle runtime helper is active.\n");
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

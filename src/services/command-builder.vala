namespace Sshuttle {

    public class CommandBuilder : Object {

        public static string[] get_effective_excludes (Profile profile, NetworkSettings settings) {
            var excludes = new GLib.GenericArray<string> ();
            var exclude_set = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);

            string host = profile.host.strip ();
            if (host != "") {
                excludes.add (host);
                exclude_set.insert (host, true);
            }

            foreach (var value in settings.exclude) {
                string network = value.strip ();
                if (network != "" && !exclude_set.contains (network)) {
                    excludes.add (network);
                    exclude_set.insert (network, true);
                }
            }

            bool has_global_v4_route = settings.routes.length == 0;
            bool has_global_v6_route = settings.routes.length == 0;
            foreach (var value in settings.routes) {
                string route = value.strip ();
                if (route == "0.0.0.0/0") {
                    has_global_v4_route = true;
                } else if (route == "::/0") {
                    has_global_v6_route = true;
                }
            }
            if (has_global_v4_route) {
                string[] local_networks = { "127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16" };
                foreach (var network in local_networks) {
                    if (!exclude_set.contains (network)) {
                        excludes.add (network);
                        exclude_set.insert (network, true);
                    }
                }
            }
            if (settings.ipv6 && (has_global_v4_route || has_global_v6_route)) {
                string[] local_networks_v6 = { "::1/128", "fc00::/7", "fe80::/10" };
                foreach (var network in local_networks_v6) {
                    if (!exclude_set.contains (network)) {
                        excludes.add (network);
                        exclude_set.insert (network, true);
                    }
                }
            }

            var result = new string[excludes.length];
            for (uint i = 0; i < excludes.length; i++) {
                result[i] = excludes[i];
            }
            return result;
        }

        public static string[] build_argv (
            Profile profile,
            NetworkSettings settings,
            int local_port = 12300
        ) throws GLib.Error {
            if (profile.host.strip () == "") {
                throw new GLib.IOError.INVALID_ARGUMENT ("Host cannot be empty");
            }

            if (local_port < 1024 || local_port > 65534 || profile.port < 0 || profile.port > 65535 ||
                profile.host.strip ().has_prefix ("-")) {
                throw new GLib.IOError.INVALID_ARGUMENT ("Invalid SSH endpoint or local port");
            }
            var argv = new GLib.GenericArray<string> ();

            argv.add ("ssh-rocket");

            // SOCKS 和 DNS 转发使用相邻端口，连接前由 TunnelManager 一并检查占用。
            argv.add ("-l");
            argv.add (settings.ipv6
                ? @"127.0.0.1:$(local_port),[::1]:$(local_port)"
                : @"127.0.0.1:$(local_port)");

            if (settings.dns) {
                argv.add ("--dns");
            }

            if (!settings.ipv6) {
                argv.add ("--disable-ipv6");
            }

            // 构造 SSH 命令，确保在后台非终端环境下能够自动接受新 Host Key，并支持读取当前普通用户的 known_hosts / agent
            var ssh_parts = new GLib.GenericArray<string> ();

            if (profile.auth_type == "password" && profile.password != "") {
                ssh_parts.add ("sshpass -e ssh");
            } else {
                ssh_parts.add ("ssh");
            }

            ssh_parts.add ("-N -T -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3");
            ssh_parts.add ("-o ForkAfterAuthentication=no -o ControlMaster=no -o ControlPath=none");
            ssh_parts.add ("-o StrictHostKeyChecking=accept-new -o ConnectTimeout=15");
            ssh_parts.add (profile.auth_type == "password" ? "-o NumberOfPasswordPrompts=1" : "-o BatchMode=yes");
            ssh_parts.add (@"-D 127.0.0.1:$(local_port)");
            if (settings.dns) {
                ssh_parts.add (@"-L 127.0.0.1:$(local_port + 1):1.1.1.1:53");
            }
            if (profile.port != 0 && profile.port != 22) {
                ssh_parts.add (@"-p $(profile.port)");
            }
            if (profile.username.strip () != "") {
                ssh_parts.add (@"-l $(GLib.Shell.quote (profile.username))");
            }
            if (settings.verbosity == "very_verbose") {
                ssh_parts.add ("-vv");
            }

            string home_dir = GLib.Environment.get_home_dir ();

            string known_hosts = GLib.Path.build_filename (home_dir, ".ssh", "known_hosts");
            if (GLib.FileUtils.test (known_hosts, GLib.FileTest.EXISTS)) {
                ssh_parts.add (@"-o UserKnownHostsFile=$(GLib.Shell.quote (known_hosts))");
            }

            string ssh_config = GLib.Path.build_filename (home_dir, ".ssh", "config");
            if (GLib.FileUtils.test (ssh_config, GLib.FileTest.EXISTS)) {
                ssh_parts.add (@"-F $(GLib.Shell.quote (ssh_config))");
            }

            if (profile.auth_type == "key" && profile.key_path.strip () != "") {
                ssh_parts.add (@"-i $(GLib.Shell.quote (profile.key_path.strip ()))");
            } else if (profile.auth_type == "agent" || profile.auth_type == "") {
                string[] default_keys = { "id_ed25519", "id_rsa", "id_ecdsa" };
                foreach (var k in default_keys) {
                    string k_path = GLib.Path.build_filename (home_dir, ".ssh", k);
                    if (GLib.FileUtils.test (k_path, GLib.FileTest.EXISTS)) {
                        ssh_parts.add (@"-o IdentityFile=$(GLib.Shell.quote (k_path))");
                    }
                }
            }

            var ssh_arr = new string[ssh_parts.length];
            for (uint i = 0; i < ssh_parts.length; i++) {
                ssh_arr[i] = ssh_parts[i];
            }
            argv.add ("-e");
            argv.add (string.joinv (" ", ssh_arr));

            argv.add ("-r");
            argv.add (profile.host.strip ());

            // 自动排除 SSH 服务器、用户配置项与全局路由下的本地网段。
            foreach (var network in get_effective_excludes (profile, settings)) {
                argv.add ("-x");
                argv.add (network);
            }

            if (settings.routes.length == 0) {
                argv.add ("0.0.0.0/0");
                if (settings.ipv6) {
                    argv.add ("::/0");
                }
            } else {
                bool has_global_v4_route = false;
                bool has_global_v6_route = false;
                foreach (var r in settings.routes) {
                    string r_trimmed = r.strip ();
                    if (r_trimmed != "") {
                        argv.add (r_trimmed);
                        has_global_v4_route = has_global_v4_route || r_trimmed == "0.0.0.0/0";
                        has_global_v6_route = has_global_v6_route || r_trimmed == "::/0";
                    }
                }
                if (settings.ipv6 && has_global_v4_route && !has_global_v6_route) {
                    argv.add ("::/0");
                }
            }

            var result = new string[argv.length];
            for (uint i = 0; i < argv.length; i++) {
                result[i] = argv[i];
            }
            return result;
        }
    }
}

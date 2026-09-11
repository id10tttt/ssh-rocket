namespace Sshuttle {

    public class CommandBuilder : Object {

        public static string[] build_argv (Profile profile, int local_port = 12300) throws GLib.Error {
            if (profile.host.strip () == "") {
                throw new GLib.IOError.INVALID_ARGUMENT ("Host cannot be empty");
            }

            var argv = new GLib.GenericArray<string> ();

            argv.add ("sshuttle");

            // 指定监听端口，便于精准管理对应 nftables 表 (sshuttle-ipv4-<port>)
            argv.add ("-l");
            argv.add (@"127.0.0.1:$(local_port)");

            if (profile.dns) {
                argv.add ("--dns");
            }

            if (profile.ipv6) {
                argv.add ("--ipv6");
            }

            // 强制采用 nftables 模式，支持内核级 cgroup v2 应用过滤
            argv.add ("--method");
            argv.add ("nft");

            // 始终启用 -v 确保输出内部 DNS 监听端口供 DnsProxy 捕获
            if (profile.verbosity == "very_verbose") {
                argv.add ("-vv");
            } else {
                argv.add ("-v");
            }

            // 构造 SSH 命令，确保在后台非终端环境下能够自动接受新 Host Key，并支持读取当前普通用户的 known_hosts / agent
            var ssh_parts = new GLib.GenericArray<string> ();

            string? auth_sock = GLib.Environment.get_variable ("SSH_AUTH_SOCK");
            if (auth_sock != null && auth_sock != "" && profile.auth_type == "agent") {
                ssh_parts.add (@"env SSH_AUTH_SOCK=$(auth_sock)");
            }

            if (profile.auth_type == "password" && profile.password != "") {
                string quoted_pwd = profile.password.replace ("'", "'\\''");
                ssh_parts.add (@"sshpass -p '$(quoted_pwd)' ssh");
            } else {
                ssh_parts.add ("ssh");
            }

            ssh_parts.add ("-o StrictHostKeyChecking=accept-new");

            string? sudo_user = GLib.Environment.get_variable ("SUDO_USER");
            string home_dir = (sudo_user != null && sudo_user != "")
                ? @"/home/$(sudo_user)"
                : GLib.Environment.get_home_dir ();

            string known_hosts = GLib.Path.build_filename (home_dir, ".ssh", "known_hosts");
            if (GLib.FileUtils.test (known_hosts, GLib.FileTest.EXISTS)) {
                ssh_parts.add (@"-o UserKnownHostsFile=$(known_hosts)");
            }

            string ssh_config = GLib.Path.build_filename (home_dir, ".ssh", "config");
            if (GLib.FileUtils.test (ssh_config, GLib.FileTest.EXISTS)) {
                ssh_parts.add (@"-F $(ssh_config)");
            }

            if (profile.auth_type == "key" && profile.key_path.strip () != "") {
                ssh_parts.add (@"-i $(profile.key_path.strip ())");
            } else if (profile.auth_type == "agent" || profile.auth_type == "") {
                string[] default_keys = { "id_ed25519", "id_rsa", "id_ecdsa" };
                foreach (var k in default_keys) {
                    string k_path = GLib.Path.build_filename (home_dir, ".ssh", k);
                    if (GLib.FileUtils.test (k_path, GLib.FileTest.EXISTS)) {
                        ssh_parts.add (@"-o IdentityFile=$(k_path)");
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
            argv.add (profile.get_ssh_target ());

            var exclude_set = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);

            // 自动排除目标服务器自身 IP / 域名，防止全局转发规则切断 SSH 连接自身
            string host_trimmed = profile.host.strip ();
            if (host_trimmed != "") {
                argv.add ("-x");
                argv.add (host_trimmed);
                exclude_set.insert (host_trimmed, true);
            }

            foreach (var exc in profile.exclude) {
                string exc_trimmed = exc.strip ();
                if (exc_trimmed != "" && !exclude_set.contains (exc_trimmed)) {
                    argv.add ("-x");
                    argv.add (exc_trimmed);
                    exclude_set.insert (exc_trimmed, true);
                }
            }

            // 当包含全局 0.0.0.0/0 路由时，默认排除回环与局域网私有网段，确保本地服务与内网访问通畅
            bool has_global_route = (profile.routes.length == 0);
            foreach (var r in profile.routes) {
                if (r.strip () == "0.0.0.0/0") {
                    has_global_route = true;
                    break;
                }
            }
            if (has_global_route) {
                string[] default_subnets = { "127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16" };
                foreach (var net in default_subnets) {
                    if (!exclude_set.contains (net)) {
                        argv.add ("-x");
                        argv.add (net);
                        exclude_set.insert (net, true);
                    }
                }
            }

            if (profile.routes.length == 0) {
                argv.add ("0.0.0.0/0");
            } else {
                foreach (var r in profile.routes) {
                    string r_trimmed = r.strip ();
                    if (r_trimmed != "") {
                        argv.add (r_trimmed);
                    }
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

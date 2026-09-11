namespace Sshuttle {

    public class CommandBuilder : Object {

        public static string[] build_argv (Profile profile, bool use_pkexec = false) throws GLib.Error {
            if (profile.host.strip () == "") {
                throw new GLib.IOError.INVALID_ARGUMENT ("Host cannot be empty");
            }

            var argv = new GLib.GenericArray<string> ();

            if (use_pkexec) {
                argv.add ("pkexec");
            }

            argv.add ("sshuttle");

            if (profile.dns) {
                argv.add ("--dns");
            }

            if (profile.ipv6) {
                argv.add ("--ipv6");
            }

            if (profile.method != "" && profile.method != "auto") {
                argv.add ("--method");
                argv.add (profile.method);
            }

            if (profile.verbosity == "verbose") {
                argv.add ("-v");
            } else if (profile.verbosity == "very_verbose") {
                argv.add ("-vv");
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

            string home_dir = GLib.Environment.get_home_dir ();
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

            // 自动排除目标服务器自身 IP / 域名，防止全局转发规则切断 SSH 连接自身
            bool host_excluded = false;
            string host_trimmed = profile.host.strip ();
            foreach (var exc in profile.exclude) {
                if (exc.strip () == host_trimmed) {
                    host_excluded = true;
                    break;
                }
            }
            if (!host_excluded && host_trimmed != "") {
                argv.add ("-x");
                argv.add (host_trimmed);
            }

            foreach (var exc in profile.exclude) {
                string exc_trimmed = exc.strip ();
                if (exc_trimmed != "" && exc_trimmed != host_trimmed) {
                    argv.add ("-x");
                    argv.add (exc_trimmed);
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

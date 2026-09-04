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

            // 处理 SSH 认证（私钥与密码）
            if (profile.auth_type == "key" && profile.key_path.strip () != "") {
                argv.add ("-e");
                argv.add (@"ssh -i $(profile.key_path.strip ())");
            } else if (profile.auth_type == "password" && profile.password != "") {
                argv.add ("-e");
                argv.add (@"sshpass -p '$(profile.password)' ssh");
            }

            argv.add ("-r");
            argv.add (profile.get_ssh_target ());

            foreach (var exc in profile.exclude) {
                string exc_trimmed = exc.strip ();
                if (exc_trimmed != "") {
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

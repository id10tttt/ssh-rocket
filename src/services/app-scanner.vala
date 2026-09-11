namespace Sshuttle {

    public class AppInfo : Object {
        public string id { get; set; default = ""; }
        public string name { get; set; default = ""; }
        public string exec_name { get; set; default = ""; }
        public string exec_line { get; set; default = ""; }
        public string icon_name { get; set; default = ""; }

        public AppInfo (string id, string name, string exec_name, string exec_line, string icon_name) {
            this.id = id;
            this.name = name;
            this.exec_name = exec_name;
            this.exec_line = exec_line;
            this.icon_name = icon_name;
        }
    }

    public class AppScanner : Object {

        public static GLib.GenericArray<AppInfo> scan_apps () {
            var list = new GLib.GenericArray<AppInfo> ();
            var seen_ids = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);

            var search_dirs = new GLib.GenericArray<string> ();
            search_dirs.add ("/usr/share/applications");
            search_dirs.add ("/usr/local/share/applications");

            string? sudo_user = GLib.Environment.get_variable ("SUDO_USER");
            string home_dir = (sudo_user != null && sudo_user != "")
                ? @"/home/$(sudo_user)"
                : GLib.Environment.get_home_dir ();

            search_dirs.add (GLib.Path.build_filename (home_dir, ".local", "share", "applications"));
            search_dirs.add ("/var/lib/flatpak/exports/share/applications");
            search_dirs.add (GLib.Path.build_filename (home_dir, ".local", "share", "flatpak", "exports", "share", "applications"));

            for (uint d = 0; d < search_dirs.length; d++) {
                string dir_path = search_dirs[d];
                if (!GLib.FileUtils.test (dir_path, GLib.FileTest.IS_DIR)) {
                    continue;
                }

                try {
                    var dir = GLib.Dir.open (dir_path);
                    string? filename = null;
                    while ((filename = dir.read_name ()) != null) {
                        if (!filename.has_suffix (".desktop")) {
                            continue;
                        }

                        if (seen_ids.contains (filename)) {
                            continue;
                        }

                        string full_path = GLib.Path.build_filename (dir_path, filename);
                        var app = parse_desktop_file (filename, full_path);
                        if (app != null) {
                            seen_ids.insert (filename, true);
                            list.add (app);
                        }
                    }
                } catch (GLib.Error e) {
                    // 目录读取失败，跳过
                }
            }

            // 按中文/应用名称拼音或字母排序
            list.sort ((a, b) => {
                return a.name.collate (b.name);
            });

            return list;
        }

        private static AppInfo? parse_desktop_file (string id, string full_path) {
            try {
                var kf = new GLib.KeyFile ();
                kf.load_from_file (full_path, GLib.KeyFileFlags.NONE);

                if (!kf.has_group ("Desktop Entry")) {
                    return null;
                }

                if (kf.has_key ("Desktop Entry", "Type")) {
                    string type_val = kf.get_string ("Desktop Entry", "Type");
                    if (type_val != "Application") {
                        return null;
                    }
                }

                if (kf.has_key ("Desktop Entry", "NoDisplay")) {
                    if (kf.get_boolean ("Desktop Entry", "NoDisplay")) {
                        return null;
                    }
                }

                string name = "";
                try {
                    name = kf.get_locale_string ("Desktop Entry", "Name");
                } catch (GLib.Error e) {
                    try {
                        name = kf.get_string ("Desktop Entry", "Name");
                    } catch (GLib.Error e2) {
                        name = id.replace (".desktop", "");
                    }
                }

                string exec_line = "";
                if (kf.has_key ("Desktop Entry", "Exec")) {
                    exec_line = kf.get_string ("Desktop Entry", "Exec");
                } else {
                    return null;
                }

                string icon_name = "";
                if (kf.has_key ("Desktop Entry", "Icon")) {
                    icon_name = kf.get_string ("Desktop Entry", "Icon");
                }

                string exec_name = extract_exec_name (exec_line);
                if (exec_name == "") {
                    return null;
                }

                return new AppInfo (id, name, exec_name, exec_line, icon_name);

            } catch (GLib.Error e) {
                return null;
            }
        }

        public static string extract_exec_name (string exec_line) {
            string cleaned = exec_line.strip ();
            if (cleaned == "") {
                return "";
            }

            try {
                string[] argv;
                GLib.Shell.parse_argv (cleaned, out argv);
                if (argv.length == 0) {
                    return "";
                }

                int idx = 0;
                // 处理类似 env FOO=BAR command
                if (argv[0] == "env" || argv[0].has_suffix ("/env")) {
                    idx = 1;
                    while (idx < argv.length && (argv[idx].contains ("=") || argv[idx].has_prefix ("-"))) {
                        idx++;
                    }
                }

                if (idx < argv.length) {
                    string token = argv[idx];
                    return GLib.Path.get_basename (token).down ();
                }
            } catch (GLib.Error e) {
                // 如果参数解析失败，采用基础分割
                string[] parts = cleaned.split (" ");
                if (parts.length > 0) {
                    return GLib.Path.get_basename (parts[0]).down ();
                }
            }

            return "";
        }
    }
}

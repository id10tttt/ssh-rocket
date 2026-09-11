namespace Sshuttle {

    public class ConfigManager : Object {
        private string config_dir;
        private string profiles_path;
        private string settings_path;

        private GLib.GenericArray<Profile> profiles;
        private string? active_profile_id = null;
        private int window_width = 460;
        private int window_height = 680;
        private bool app_proxy_enabled = false;
        private GLib.GenericArray<string> proxy_apps;
        private GLib.GenericArray<DomainRule> domain_rules;
        private string domain_default_policy = "direct";

        public signal void app_rules_changed ();
        public signal void domain_rules_changed ();

        public ConfigManager () {
            this.proxy_apps = new GLib.GenericArray<string> ();
            this.domain_rules = new GLib.GenericArray<DomainRule> ();

            string? env_dir = GLib.Environment.get_variable ("SSHUTTLE_CONFIG_DIR");
            string? sudo_user = GLib.Environment.get_variable ("SUDO_USER");

            if (env_dir != null && env_dir != "") {
                this.config_dir = env_dir;
            } else if (sudo_user != null && sudo_user != "") {
                this.config_dir = GLib.Path.build_filename (
                    "/home",
                    sudo_user,
                    ".config",
                    "sshuttle-gui"
                );
            } else {
                this.config_dir = GLib.Path.build_filename (
                    GLib.Environment.get_user_config_dir (),
                    "sshuttle-gui"
                );
            }

            this.profiles_path = GLib.Path.build_filename (this.config_dir, "profiles.json");
            this.settings_path = GLib.Path.build_filename (this.config_dir, "settings.json");

            this.profiles = new GLib.GenericArray<Profile> ();

            this.ensure_dir ();
            this.load ();
        }

        private void ensure_dir () {
            try {
                var file = GLib.File.new_for_path (this.config_dir);
                if (!file.query_exists ()) {
                    file.make_directory_with_parents ();
                }
            } catch (GLib.Error e) {
                // 忽略只读系统等降级异常
            }
        }

        public void load () {
            this.profiles.remove_range (0, this.profiles.length);

            if (GLib.FileUtils.test (this.profiles_path, GLib.FileTest.EXISTS)) {
                try {
                    var parser = new Json.Parser ();
                    parser.load_from_file (this.profiles_path);
                    var root = parser.get_root ();
                    if (root != null && root.get_node_type () == Json.NodeType.OBJECT) {
                        var obj = root.get_object ();
                        if (obj.has_member ("active_profile_id")) {
                            this.active_profile_id = obj.get_string_member ("active_profile_id");
                        }
                        if (obj.has_member ("profiles")) {
                            var arr = obj.get_array_member ("profiles");
                            arr.foreach_element ((array, index, element_node) => {
                                if (element_node.get_node_type () == Json.NodeType.OBJECT) {
                                    var p = Profile.deserialize (element_node.get_object ());
                                    if (p.name != "Example VPS") {
                                        this.profiles.add (p);
                                    }
                                }
                            });
                        }
                    }
                } catch (GLib.Error e) {
                    warning ("Failed to load profiles: %s", e.message);
                }
            }

            if (this.profiles.length > 0 && (this.active_profile_id == null || this.get_active_profile () == null)) {
                this.active_profile_id = this.profiles[0].id;
            } else if (this.profiles.length == 0) {
                this.active_profile_id = null;
            }

            if (GLib.FileUtils.test (this.settings_path, GLib.FileTest.EXISTS)) {
                try {
                    var parser = new Json.Parser ();
                    parser.load_from_file (this.settings_path);
                    var root = parser.get_root ();
                    if (root != null && root.get_node_type () == Json.NodeType.OBJECT) {
                        var obj = root.get_object ();
                        if (obj.has_member ("window_width")) {
                            this.window_width = (int) obj.get_int_member ("window_width");
                        }
                        if (obj.has_member ("window_height")) {
                            this.window_height = (int) obj.get_int_member ("window_height");
                        }
                        if (obj.has_member ("app_proxy_enabled")) {
                            this.app_proxy_enabled = obj.get_boolean_member ("app_proxy_enabled");
                        }
                        if (obj.has_member ("proxy_apps")) {
                            this.proxy_apps.remove_range (0, this.proxy_apps.length);
                            var arr = obj.get_array_member ("proxy_apps");
                            arr.foreach_element ((array, index, element_node) => {
                                this.proxy_apps.add (element_node.get_string ());
                            });
                        }
                        if (obj.has_member ("domain_default_policy")) {
                            this.domain_default_policy = obj.get_string_member ("domain_default_policy");
                        }
                        if (obj.has_member ("domain_rules")) {
                            this.domain_rules.remove_range (0, this.domain_rules.length);
                            var arr = obj.get_array_member ("domain_rules");
                            arr.foreach_element ((array, index, element_node) => {
                                if (element_node.get_node_type () == Json.NodeType.OBJECT) {
                                    this.domain_rules.add (DomainRule.deserialize (element_node.get_object ()));
                                }
                            });
                        }
                    }
                } catch (GLib.Error e) {
                    // 忽略设置读取异常
                }
            }
        }

        public void save_profiles () {
            this.ensure_dir ();
            var builder = new Json.Builder ();
            builder.begin_object ();

            builder.set_member_name ("active_profile_id");
            if (this.active_profile_id != null) {
                builder.add_string_value (this.active_profile_id);
            } else {
                builder.add_null_value ();
            }

            builder.set_member_name ("profiles");
            builder.begin_array ();
            for (uint i = 0; i < this.profiles.length; i++) {
                var p = this.profiles[i];
                builder.add_value (p.serialize ());
            }
            builder.end_array ();

            builder.end_object ();

            var generator = new Json.Generator ();
            generator.set_root (builder.get_root ());
            generator.set_pretty (true);

            try {
                generator.to_file (this.profiles_path);
                this.fix_ownership (this.profiles_path);
            } catch (GLib.Error e) {
                warning ("Failed to save profiles: %s", e.message);
            }
        }

        public Profile[] get_profiles () {
            var arr = new Profile[this.profiles.length];
            for (uint i = 0; i < this.profiles.length; i++) {
                arr[i] = this.profiles[i];
            }
            return arr;
        }

        public Profile? get_active_profile () {
            if (this.active_profile_id != null) {
                for (uint i = 0; i < this.profiles.length; i++) {
                    if (this.profiles[i].id == this.active_profile_id) {
                        return this.profiles[i];
                    }
                }
            }
            if (this.profiles.length > 0) {
                return this.profiles[0];
            }
            return null;
        }

        public void set_active_profile (string profile_id) {
            this.active_profile_id = profile_id;
            this.save_profiles ();
        }

        public void save_profile (Profile profile) {
            bool found = false;
            for (uint i = 0; i < this.profiles.length; i++) {
                if (this.profiles[i].id == profile.id) {
                    this.profiles[i] = profile;
                    found = true;
                    break;
                }
            }
            if (!found) {
                this.profiles.add (profile);
            }
            if (this.active_profile_id == null) {
                this.active_profile_id = profile.id;
            }
            this.save_profiles ();
        }

        public bool delete_profile (string profile_id) {
            for (uint i = 0; i < this.profiles.length; i++) {
                if (this.profiles[i].id == profile_id) {
                    this.profiles.remove_index (i);
                    if (this.active_profile_id == profile_id) {
                        this.active_profile_id = (this.profiles.length > 0) ? this.profiles[0].id : null;
                    }
                    this.save_profiles ();
                    return true;
                }
            }
            return false;
        }

        public int get_window_width () {
            return this.window_width;
        }

        public int get_window_height () {
            return this.window_height;
        }

        public void save_settings () {
            this.ensure_dir ();
            var builder = new Json.Builder ();
            builder.begin_object ();

            builder.set_member_name ("window_width");
            builder.add_int_value (this.window_width);

            builder.set_member_name ("window_height");
            builder.add_int_value (this.window_height);

            builder.set_member_name ("app_proxy_enabled");
            builder.add_boolean_value (this.app_proxy_enabled);

            builder.set_member_name ("proxy_apps");
            builder.begin_array ();
            for (uint i = 0; i < this.proxy_apps.length; i++) {
                builder.add_string_value (this.proxy_apps[i]);
            }
            builder.end_array ();

            builder.set_member_name ("domain_default_policy");
            builder.add_string_value (this.domain_default_policy);

            builder.set_member_name ("domain_rules");
            builder.begin_array ();
            for (uint i = 0; i < this.domain_rules.length; i++) {
                builder.add_value (this.domain_rules[i].serialize ());
            }
            builder.end_array ();

            builder.end_object ();

            var generator = new Json.Generator ();
            generator.set_root (builder.get_root ());
            generator.set_pretty (true);
            try {
                generator.to_file (this.settings_path);
                this.fix_ownership (this.settings_path);
            } catch (GLib.Error e) {
                // 忽略设置保存异常
            }
        }

        public void set_window_size (int w, int h) {
            this.window_width = w;
            this.window_height = h;
            this.save_settings ();
        }

        public bool get_app_proxy_enabled () {
            return this.app_proxy_enabled;
        }

        public void set_app_proxy_enabled (bool enabled) {
            if (this.app_proxy_enabled != enabled) {
                this.app_proxy_enabled = enabled;
                this.save_settings ();
                this.app_rules_changed ();
            }
        }

        public string[] get_proxy_apps () {
            var arr = new string[this.proxy_apps.length];
            for (uint i = 0; i < this.proxy_apps.length; i++) {
                arr[i] = this.proxy_apps[i];
            }
            return arr;
        }

        public bool is_app_proxied (string app_id) {
            for (uint i = 0; i < this.proxy_apps.length; i++) {
                if (this.proxy_apps[i] == app_id) {
                    return true;
                }
            }
            return false;
        }

        public void set_app_proxied (string app_id, bool proxied) {
            bool changed = false;
            if (proxied) {
                if (!this.is_app_proxied (app_id)) {
                    this.proxy_apps.add (app_id);
                    changed = true;
                }
            } else {
                for (uint i = 0; i < this.proxy_apps.length; i++) {
                    if (this.proxy_apps[i] == app_id) {
                        this.proxy_apps.remove_index (i);
                        changed = true;
                        break;
                    }
                }
            }

            if (changed) {
                this.save_settings ();
                this.app_rules_changed ();
            }
        }

        public string get_domain_default_policy () {
            return this.domain_default_policy;
        }

        public void set_domain_default_policy (string policy) {
            string p = (policy.down () == "proxy") ? "proxy" : "direct";
            if (this.domain_default_policy != p) {
                this.domain_default_policy = p;
                this.save_settings ();
                this.domain_rules_changed ();
            }
        }

        public DomainRule[] get_domain_rules () {
            var arr = new DomainRule[this.domain_rules.length];
            for (uint i = 0; i < this.domain_rules.length; i++) {
                arr[i] = this.domain_rules[i];
            }
            return arr;
        }

        public void add_domain_rule (string pattern, string action = "proxy") {
            string p = pattern.strip ().down ();
            if (p == "") {
                return;
            }

            // 如果已有相同模式，先移除旧的
            this.remove_domain_rule (p);

            this.domain_rules.add (new DomainRule (p, action));
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void remove_domain_rule (string pattern) {
            string p = pattern.strip ().down ();
            bool removed = false;
            for (uint i = 0; i < this.domain_rules.length; i++) {
                if (this.domain_rules[i].pattern == p) {
                    this.domain_rules.remove_index (i);
                    removed = true;
                    break;
                }
            }
            if (removed) {
                this.save_settings ();
                this.domain_rules_changed ();
            }
        }

        public void set_domain_rules (DomainRule[] rules, string default_policy = "") {
            this.domain_rules.remove_range (0, this.domain_rules.length);
            foreach (var r in rules) {
                this.domain_rules.add (r);
            }
            if (default_policy != "") {
                this.domain_default_policy = (default_policy.down () == "proxy") ? "proxy" : "direct";
            }
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void clear_domain_rules () {
            this.domain_rules.remove_range (0, this.domain_rules.length);
            this.save_settings ();
            this.domain_rules_changed ();
        }

        private void fix_ownership (string file_path) {
            string? sudo_uid_str = GLib.Environment.get_variable ("SUDO_UID");
            string? sudo_gid_str = GLib.Environment.get_variable ("SUDO_GID");
            if (sudo_uid_str != null && sudo_gid_str != null) {
                int uid = int.parse (sudo_uid_str);
                int gid = int.parse (sudo_gid_str);
                if (uid > 0) {
                    Posix.chown (file_path, (Posix.uid_t) uid, (Posix.gid_t) gid);
                    Posix.chown (this.config_dir, (Posix.uid_t) uid, (Posix.gid_t) gid);
                }
            }
        }
    }
}

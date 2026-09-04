namespace Sshuttle {

    public class ConfigManager : Object {
        private string config_dir;
        private string profiles_path;
        private string settings_path;

        private GLib.GenericArray<Profile> profiles;
        private string? active_profile_id = null;
        private int window_width = 460;
        private int window_height = 680;

        public ConfigManager () {
            string? env_dir = GLib.Environment.get_variable ("SSHUTTLE_CONFIG_DIR");
            if (env_dir != null && env_dir != "") {
                this.config_dir = env_dir;
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

        public void set_window_size (int w, int h) {
            this.window_width = w;
            this.window_height = h;

            var builder = new Json.Builder ();
            builder.begin_object ();
            builder.set_member_name ("window_width");
            builder.add_int_value (w);
            builder.set_member_name ("window_height");
            builder.add_int_value (h);
            builder.end_object ();

            var generator = new Json.Generator ();
            generator.set_root (builder.get_root ());
            try {
                generator.to_file (this.settings_path);
            } catch (GLib.Error e) {
                // 忽略设置保存异常
            }
        }
    }
}

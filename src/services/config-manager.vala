namespace Sshuttle {

    public class AppTrafficStats : Object {
        public string app_id { get; set; }
        public uint64 bytes_uploaded { get; set; default = 0; }
        public uint64 bytes_downloaded { get; set; default = 0; }

        public AppTrafficStats (string app_id, uint64 uploaded = 0, uint64 downloaded = 0) {
            this.app_id = app_id;
            this.bytes_uploaded = uploaded;
            this.bytes_downloaded = downloaded;
        }
    }

    public class RuleSourceInfo : Object {
        public string id { get; set; default = ""; }
        public string name { get; set; default = "Imported Configuration"; }
        public string url { get; set; default = ""; }
        public int64 updated_at { get; set; default = 0; }
        public string cache_file { get; set; default = ""; }
        public string default_policy { get; set; default = "proxy"; }

        public RuleSourceInfo () {
            this.id = GLib.Uuid.string_random ();
        }

        public Json.Node serialize () {
            var builder = new Json.Builder ();
            builder.begin_object ();
            builder.set_member_name ("id");
            builder.add_string_value (this.id);
            builder.set_member_name ("name");
            builder.add_string_value (this.name);
            builder.set_member_name ("url");
            builder.add_string_value (this.url);
            builder.set_member_name ("updated_at");
            builder.add_int_value (this.updated_at);
            builder.set_member_name ("cache_file");
            builder.add_string_value (this.cache_file);
            builder.set_member_name ("default_policy");
            builder.add_string_value (this.default_policy);
            builder.end_object ();
            return builder.get_root ();
        }

        public static RuleSourceInfo deserialize (Json.Object obj) {
            var info = new RuleSourceInfo ();
            if (obj.has_member ("id")) info.id = obj.get_string_member ("id");
            if (obj.has_member ("name")) info.name = obj.get_string_member ("name");
            if (obj.has_member ("url")) info.url = obj.get_string_member ("url");
            if (obj.has_member ("updated_at")) info.updated_at = obj.get_int_member ("updated_at");
            if (obj.has_member ("cache_file")) info.cache_file = obj.get_string_member ("cache_file");
            if (obj.has_member ("default_policy")) {
                info.default_policy = obj.get_string_member ("default_policy") == "direct"
                    ? "direct" : "proxy";
            }
            return info;
        }
    }

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
        private GLib.GenericArray<string> blocked_apps;
        private GLib.GenericArray<string> blocked_processes;
        private GLib.GenericArray<DomainRule> domain_rules;
        private GLib.GenericArray<DomainRule> imported_domain_rules;
        private string domain_default_policy = "proxy";
        private string rule_source_url = "";
        private string rule_source_name = "";
        private int64 rule_source_updated_at = 0;
        private string rule_cache_path;
        private GLib.GenericArray<RuleSourceInfo> rule_sources;
        private string active_rule_source_id = "";
        private DomainRuleMatcher domain_rule_matcher;
        private GLib.HashTable<string, AppTrafficStats> app_traffic;
        private NetworkSettings network_settings;

        public signal void app_rules_changed ();
        public signal void domain_rules_changed ();
        public signal void blacklist_changed ();
        public signal void traffic_stats_changed ();
        public signal void network_settings_changed ();

        public ConfigManager () {
            this.proxy_apps = new GLib.GenericArray<string> ();
            this.blocked_apps = new GLib.GenericArray<string> ();
            this.blocked_processes = new GLib.GenericArray<string> ();
            this.domain_rules = new GLib.GenericArray<DomainRule> ();
            this.imported_domain_rules = new GLib.GenericArray<DomainRule> ();
            this.rule_sources = new GLib.GenericArray<RuleSourceInfo> ();
            this.domain_rule_matcher = new DomainRuleMatcher ({}, this.domain_default_policy);
            this.app_traffic = new GLib.HashTable<string, AppTrafficStats> (GLib.str_hash, GLib.str_equal);
            this.network_settings = new NetworkSettings ();

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
            this.rule_cache_path = GLib.Path.build_filename (this.config_dir, "imported-rules.conf");

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
            bool has_global_network_settings = false;
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
                        if (obj.has_member ("routes")) {
                            this.network_settings.routes = this.read_string_array (obj, "routes");
                            has_global_network_settings = true;
                        }
                        if (obj.has_member ("exclude_networks")) {
                            this.network_settings.exclude = this.read_string_array (obj, "exclude_networks");
                            has_global_network_settings = true;
                        }
                        if (obj.has_member ("dns")) {
                            this.network_settings.dns = obj.get_boolean_member ("dns");
                            has_global_network_settings = true;
                        }
                        if (obj.has_member ("ipv6")) {
                            this.network_settings.ipv6 = obj.get_boolean_member ("ipv6");
                            has_global_network_settings = true;
                        }
                        if (obj.has_member ("verbosity")) {
                            this.network_settings.verbosity = obj.get_string_member ("verbosity");
                            has_global_network_settings = true;
                        }
                        if (obj.has_member ("auto_connect")) {
                            this.network_settings.auto_connect = obj.get_boolean_member ("auto_connect");
                            has_global_network_settings = true;
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
                        if (obj.has_member ("blocked_apps")) {
                            this.blocked_apps.remove_range (0, this.blocked_apps.length);
                            var arr = obj.get_array_member ("blocked_apps");
                            arr.foreach_element ((array, index, element_node) => {
                                this.blocked_apps.add (element_node.get_string ());
                            });
                        }
                        if (obj.has_member ("blocked_processes")) {
                            this.blocked_processes.remove_range (0, this.blocked_processes.length);
                            var arr = obj.get_array_member ("blocked_processes");
                            arr.foreach_element ((array, index, element_node) => {
                                this.blocked_processes.add (element_node.get_string ());
                            });
                        }
                        if (obj.has_member ("domain_default_policy")) {
                            string policy = obj.get_string_member ("domain_default_policy").down ();
                            this.domain_default_policy = policy == "direct" ? "direct" : "proxy";
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
                        if (obj.has_member ("rule_source_url")) {
                            this.rule_source_url = obj.get_string_member ("rule_source_url");
                        }
                        if (obj.has_member ("rule_source_name")) {
                            this.rule_source_name = obj.get_string_member ("rule_source_name");
                        }
                        if (obj.has_member ("rule_source_updated_at")) {
                            this.rule_source_updated_at = obj.get_int_member ("rule_source_updated_at");
                        }
                        if (obj.has_member ("active_rule_source_id")) {
                            this.active_rule_source_id = obj.get_string_member ("active_rule_source_id");
                        }
                        if (obj.has_member ("rule_sources")) {
                            this.rule_sources.remove_range (0, this.rule_sources.length);
                            var sources = obj.get_array_member ("rule_sources");
                            sources.foreach_element ((array, index, element_node) => {
                                if (element_node.get_node_type () == Json.NodeType.OBJECT) {
                                    this.rule_sources.add (RuleSourceInfo.deserialize (element_node.get_object ()));
                                }
                            });
                        }
                        if (obj.has_member ("app_traffic")) {
                            this.app_traffic.remove_all ();
                            var traffic_obj = obj.get_object_member ("app_traffic");
                            var members = traffic_obj.get_members ();
                            foreach (var member in members) {
                                if (traffic_obj.has_member (member)) {
                                    var item = traffic_obj.get_object_member (member);
                                    uint64 up = (uint64) item.get_int_member ("uploaded");
                                    uint64 down = (uint64) item.get_int_member ("downloaded");
                                    this.app_traffic.insert (member, new AppTrafficStats (member, up, down));
                                }
                            }
                        }
                    }
                } catch (GLib.Error e) {
                    // 忽略设置读取异常
                }
            }

            if (!has_global_network_settings) {
                var legacy_profile = this.get_active_profile ();
                if (legacy_profile != null) {
                    this.network_settings.routes = legacy_profile.legacy_routes;
                    this.network_settings.exclude = legacy_profile.legacy_exclude;
                    this.network_settings.dns = legacy_profile.legacy_dns;
                    this.network_settings.ipv6 = legacy_profile.legacy_ipv6;
                    this.network_settings.verbosity = legacy_profile.legacy_verbosity;
                    this.network_settings.auto_connect = legacy_profile.legacy_auto_connect;
                }
            }

            this.load_rule_cache ();
            this.migrate_legacy_rule_source ();
            this.rebuild_domain_rule_matcher ();
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

            builder.set_member_name ("routes");
            this.write_string_array (builder, this.network_settings.routes);

            builder.set_member_name ("exclude_networks");
            this.write_string_array (builder, this.network_settings.exclude);

            builder.set_member_name ("dns");
            builder.add_boolean_value (this.network_settings.dns);
            builder.set_member_name ("ipv6");
            builder.add_boolean_value (this.network_settings.ipv6);
            builder.set_member_name ("verbosity");
            builder.add_string_value (this.network_settings.verbosity);
            builder.set_member_name ("auto_connect");
            builder.add_boolean_value (this.network_settings.auto_connect);

            builder.set_member_name ("app_proxy_enabled");
            builder.add_boolean_value (this.app_proxy_enabled);

            builder.set_member_name ("proxy_apps");
            builder.begin_array ();
            for (uint i = 0; i < this.proxy_apps.length; i++) {
                builder.add_string_value (this.proxy_apps[i]);
            }
            builder.end_array ();

            builder.set_member_name ("blocked_apps");
            builder.begin_array ();
            for (uint i = 0; i < this.blocked_apps.length; i++) {
                builder.add_string_value (this.blocked_apps[i]);
            }
            builder.end_array ();

            builder.set_member_name ("blocked_processes");
            builder.begin_array ();
            for (uint i = 0; i < this.blocked_processes.length; i++) {
                builder.add_string_value (this.blocked_processes[i]);
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

            builder.set_member_name ("rule_source_url");
            builder.add_string_value (this.rule_source_url);
            builder.set_member_name ("rule_source_name");
            builder.add_string_value (this.rule_source_name);
            builder.set_member_name ("rule_source_updated_at");
            builder.add_int_value (this.rule_source_updated_at);

            builder.set_member_name ("active_rule_source_id");
            builder.add_string_value (this.active_rule_source_id);
            builder.set_member_name ("rule_sources");
            builder.begin_array ();
            for (uint i = 0; i < this.rule_sources.length; i++) {
                builder.add_value (this.rule_sources[i].serialize ());
            }
            builder.end_array ();

            builder.set_member_name ("app_traffic");
            builder.begin_object ();
            var iter = GLib.HashTableIter<string, AppTrafficStats> (this.app_traffic);
            string k;
            AppTrafficStats v;
            while (iter.next (out k, out v)) {
                builder.set_member_name (k);
                builder.begin_object ();
                builder.set_member_name ("uploaded");
                builder.add_int_value ((int64) v.bytes_uploaded);
                builder.set_member_name ("downloaded");
                builder.add_int_value ((int64) v.bytes_downloaded);
                builder.end_object ();
            }
            builder.end_object ();

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

        public void get_app_traffic (string app_id, out uint64 uploaded, out uint64 downloaded) {
            var stats = this.app_traffic.lookup (app_id);
            if (stats != null) {
                uploaded = stats.bytes_uploaded;
                downloaded = stats.bytes_downloaded;
            } else {
                uploaded = 0;
                downloaded = 0;
            }
        }

        public void add_app_traffic (string app_id, uint64 up_delta, uint64 down_delta) {
            if (up_delta == 0 && down_delta == 0) {
                return;
            }
            var stats = this.app_traffic.lookup (app_id);
            if (stats == null) {
                stats = new AppTrafficStats (app_id, up_delta, down_delta);
                this.app_traffic.insert (app_id, stats);
            } else {
                stats.bytes_uploaded += up_delta;
                stats.bytes_downloaded += down_delta;
            }
            this.traffic_stats_changed ();
        }

        public void get_total_traffic (out uint64 total_uploaded, out uint64 total_downloaded) {
            uint64 up = 0;
            uint64 down = 0;
            var iter = GLib.HashTableIter<string, AppTrafficStats> (this.app_traffic);
            AppTrafficStats v;
            while (iter.next (null, out v)) {
                up += v.bytes_uploaded;
                down += v.bytes_downloaded;
            }
            total_uploaded = up;
            total_downloaded = down;
        }

        public void reset_traffic_stats () {
            this.app_traffic.remove_all ();
            this.save_settings ();
            this.traffic_stats_changed ();
        }

        /**
         * 重置全局设置、规则与统计，同时保留连接 Profile 和窗口尺寸。
         */
        public void reset_rules_and_settings () {
            this.network_settings = new NetworkSettings ();
            this.app_proxy_enabled = false;
            this.proxy_apps.remove_range (0, this.proxy_apps.length);
            this.blocked_apps.remove_range (0, this.blocked_apps.length);
            this.blocked_processes.remove_range (0, this.blocked_processes.length);
            this.domain_rules.remove_range (0, this.domain_rules.length);
            this.imported_domain_rules.remove_range (0, this.imported_domain_rules.length);
            this.domain_default_policy = "proxy";
            this.rule_source_url = "";
            this.rule_source_name = "";
            this.rule_source_updated_at = 0;
            for (uint i = 0; i < this.rule_sources.length; i++) {
                GLib.FileUtils.remove (this.get_rule_source_path (this.rule_sources[i]));
            }
            this.rule_sources.remove_range (0, this.rule_sources.length);
            this.active_rule_source_id = "";
            GLib.FileUtils.remove (this.rule_cache_path);
            this.rebuild_domain_rule_matcher ();
            this.app_traffic.remove_all ();
            this.save_settings ();

            this.app_rules_changed ();
            this.domain_rules_changed ();
            this.blacklist_changed ();
            this.traffic_stats_changed ();
            this.network_settings_changed ();
        }

        public NetworkSettings get_network_settings () {
            return this.network_settings;
        }

        /**
         * 保存所有连接共用的路由与运行设置。
         */
        public void set_network_settings (NetworkSettings settings) {
            this.network_settings.routes = settings.routes;
            this.network_settings.exclude = settings.exclude;
            this.network_settings.dns = settings.dns;
            this.network_settings.ipv6 = settings.ipv6;
            this.network_settings.verbosity = settings.verbosity;
            this.network_settings.auto_connect = settings.auto_connect;
            this.save_settings ();
            this.network_settings_changed ();
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

        public string[] get_blocked_apps () {
            var arr = new string[this.blocked_apps.length];
            for (uint i = 0; i < this.blocked_apps.length; i++) {
                arr[i] = this.blocked_apps[i];
            }
            return arr;
        }

        public bool is_app_blocked (string app_id) {
            for (uint i = 0; i < this.blocked_apps.length; i++) {
                if (this.blocked_apps[i] == app_id) {
                    return true;
                }
            }
            return false;
        }

        public void set_app_blocked (string app_id, bool blocked) {
            bool changed = false;
            if (blocked) {
                if (!this.is_app_blocked (app_id)) {
                    this.blocked_apps.add (app_id);
                    changed = true;
                }
            } else {
                for (uint i = 0; i < this.blocked_apps.length; i++) {
                    if (this.blocked_apps[i] == app_id) {
                        this.blocked_apps.remove_index (i);
                        changed = true;
                        break;
                    }
                }
            }

            if (changed) {
                this.save_settings ();
                this.blacklist_changed ();
            }
        }

        public string[] get_blocked_processes () {
            var arr = new string[this.blocked_processes.length];
            for (uint i = 0; i < this.blocked_processes.length; i++) {
                arr[i] = this.blocked_processes[i];
            }
            return arr;
        }

        public bool is_process_blocked (string proc_name) {
            string p = proc_name.strip ().down ();
            for (uint i = 0; i < this.blocked_processes.length; i++) {
                if (this.blocked_processes[i] == p) {
                    return true;
                }
            }
            return false;
        }

        public void add_blocked_process (string proc_name) {
            string p = proc_name.strip ().down ();
            if (p == "") return;
            if (!this.is_process_blocked (p)) {
                this.blocked_processes.add (p);
                this.save_settings ();
                this.blacklist_changed ();
            }
        }

        public void remove_blocked_process (string proc_name) {
            string p = proc_name.strip ().down ();
            for (uint i = 0; i < this.blocked_processes.length; i++) {
                if (this.blocked_processes[i] == p) {
                    this.blocked_processes.remove_index (i);
                    this.save_settings ();
                    this.blacklist_changed ();
                    break;
                }
            }
        }

        public string get_domain_default_policy () {
            return this.domain_default_policy;
        }

        public void set_domain_default_policy (string policy) {
            string p = (policy.down () == "proxy") ? "proxy" : "direct";
            if (this.domain_default_policy != p) {
                this.domain_default_policy = p;
                var source = this.get_active_rule_source ();
                if (source != null) source.default_policy = p;
                this.rebuild_domain_rule_matcher ();
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

        public DomainRule[] get_effective_domain_rules () {
            var rules = new DomainRule[this.domain_rules.length + this.imported_domain_rules.length];
            uint index = 0;
            for (uint i = 0; i < this.domain_rules.length; i++) rules[index++] = this.domain_rules[i];
            for (uint i = 0; i < this.imported_domain_rules.length; i++) rules[index++] = this.imported_domain_rules[i];
            return rules;
        }

        public DomainRule[] get_imported_domain_rules () {
            var rules = new DomainRule[this.imported_domain_rules.length];
            for (uint i = 0; i < this.imported_domain_rules.length; i++) {
                rules[i] = this.imported_domain_rules[i];
            }
            return rules;
        }

        public DomainRule[] get_network_rules () {
            var result = new GLib.GenericArray<DomainRule> ();
            foreach (var rule in this.get_effective_domain_rules ()) {
                string address_text = rule.pattern.split ("/", 2)[0];
                if (rule.rule_type == "ip-cidr" ||
                    new GLib.InetAddress.from_string (address_text) != null) {
                    result.add (rule);
                }
            }
            var rules = new DomainRule[result.length];
            for (uint i = 0; i < result.length; i++) rules[i] = result[i];
            return rules;
        }

        public string resolve_domain_action (string? domain, out bool matched = null) {
            return this.domain_rule_matcher.resolve (domain, out matched);
        }

        /** 显式规则优先，其次是应用代理，最后使用配置默认策略。 */
        public string resolve_traffic_action (
            string? domain,
            bool app_proxy_enabled,
            out bool matched = null
        ) {
            string action = this.domain_rule_matcher.resolve (domain, out matched);
            return matched ? action : (app_proxy_enabled ? "proxy" : this.domain_default_policy);
        }

        public void add_domain_rule (
            string pattern,
            string action = "proxy",
            string rule_type = "legacy"
        ) {
            string p = pattern.strip ().down ();
            if (p == "") {
                return;
            }

            // 如果已有相同模式，先移除旧的
            this.remove_domain_rule (p);

            this.domain_rules.add (new DomainRule (p, action, rule_type));
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void update_domain_rule (
            string old_pattern,
            string new_pattern,
            string new_action,
            string new_rule_type = "legacy"
        ) {
            string op = old_pattern.strip ().down ();
            string np = new_pattern.strip ().down ();
            string requested_action = new_action.strip ().down ();
            string na = requested_action == "reject" ? "reject" :
                (requested_action == "proxy" ? "proxy" : "direct");
            if (np == "") {
                return;
            }

            bool found = false;
            for (uint i = 0; i < this.domain_rules.length; i++) {
                if (this.domain_rules[i].pattern == op) {
                    this.domain_rules[i] = new DomainRule (np, na, new_rule_type);
                    found = true;
                    break;
                }
            }

            if (!found) {
                this.add_domain_rule (np, na, new_rule_type);
                return;
            }

            this.save_settings ();
            this.rebuild_domain_rule_matcher ();
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
                this.rebuild_domain_rule_matcher ();
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
                var source = this.get_active_rule_source ();
                if (source != null) source.default_policy = this.domain_default_policy;
            }
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void clear_domain_rules () {
            this.domain_rules.remove_range (0, this.domain_rules.length);
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void set_imported_rule_source (RuleImportResult result, string url, string name) throws GLib.Error {
            var info = this.get_active_rule_source ();
            if (info == null) {
                this.add_imported_rule_source (result, url, name);
                return;
            }
            string old_url = info.url;
            string old_name = info.name;
            info.url = url;
            info.name = name;
            try {
                this.store_rule_source (info, result);
            } catch (GLib.Error e) {
                info.url = old_url;
                info.name = old_name;
                throw e;
            }
        }

        /** 新增并启用一份独立配置，自定义规则不随配置切换。 */
        public void add_imported_rule_source (
            RuleImportResult result,
            string url,
            string name
        ) throws GLib.Error {
            var info = new RuleSourceInfo ();
            info.url = url;
            info.name = name != "" ? name : "Imported Configuration";
            info.cache_file = @"rules-$(info.id).conf";
            string path = this.get_rule_source_path (info);
            GLib.FileUtils.set_contents (path, result.to_cache ());
            this.fix_ownership (path);
            info.updated_at = new GLib.DateTime.now_local ().to_unix ();
            info.default_policy = result.default_policy == "proxy" ? "proxy" : "direct";
            this.rule_sources.add (info);
            this.active_rule_source_id = info.id;
            this.load_rule_cache ();
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public void clear_imported_rule_source () {
            var active = this.get_active_rule_source ();
            if (active != null) {
                GLib.FileUtils.remove (this.get_rule_source_path (active));
                for (uint i = 0; i < this.rule_sources.length; i++) {
                    if (this.rule_sources[i].id == active.id) {
                        this.rule_sources.remove_index (i);
                        break;
                    }
                }
            }
            this.active_rule_source_id = this.rule_sources.length > 0 ? this.rule_sources[0].id : "";
            this.load_rule_cache ();
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        public RuleSourceInfo[] get_rule_sources () {
            var sources = new RuleSourceInfo[this.rule_sources.length];
            for (uint i = 0; i < this.rule_sources.length; i++) sources[i] = this.rule_sources[i];
            return sources;
        }

        public string get_active_rule_source_id () {
            return this.active_rule_source_id;
        }

        public void set_active_rule_source (string source_id) {
            if (source_id == this.active_rule_source_id) return;
            bool found = false;
            for (uint i = 0; i < this.rule_sources.length; i++) {
                if (this.rule_sources[i].id == source_id) {
                    found = true;
                    break;
                }
            }
            if (!found) return;
            this.active_rule_source_id = source_id;
            this.load_rule_cache ();
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        private void store_rule_source (RuleSourceInfo info, RuleImportResult result) throws GLib.Error {
            string path = this.get_rule_source_path (info);
            GLib.FileUtils.set_contents (path, result.to_cache ());
            this.fix_ownership (path);
            info.updated_at = new GLib.DateTime.now_local ().to_unix ();
            info.default_policy = result.default_policy == "proxy" ? "proxy" : "direct";
            this.active_rule_source_id = info.id;
            this.load_rule_cache ();
            this.rebuild_domain_rule_matcher ();
            this.save_settings ();
            this.domain_rules_changed ();
        }

        private RuleSourceInfo? get_active_rule_source () {
            for (uint i = 0; i < this.rule_sources.length; i++) {
                if (this.rule_sources[i].id == this.active_rule_source_id) return this.rule_sources[i];
            }
            return null;
        }

        private string get_rule_source_path (RuleSourceInfo info) {
            string filename = info.cache_file != ""
                ? GLib.Path.get_basename (info.cache_file)
                : @"rules-$(info.id).conf";
            return GLib.Path.build_filename (this.config_dir, filename);
        }

        private void sync_active_rule_source_metadata (RuleSourceInfo? info) {
            this.imported_domain_rules.remove_range (0, this.imported_domain_rules.length);
            this.rule_source_url = info != null ? info.url : "";
            this.rule_source_name = info != null ? info.name : "";
            this.rule_source_updated_at = info != null ? info.updated_at : 0;
            if (info != null) this.domain_default_policy = info.default_policy;
        }

        public string get_rule_source_url () { return this.rule_source_url; }
        public string get_rule_source_name () { return this.rule_source_name; }
        public int64 get_rule_source_updated_at () { return this.rule_source_updated_at; }
        public uint get_imported_rule_count () { return this.imported_domain_rules.length; }

        public void get_imported_rule_counts (out uint direct, out uint proxy, out uint reject) {
            direct = proxy = reject = 0;
            for (uint i = 0; i < this.imported_domain_rules.length; i++) {
                switch (this.imported_domain_rules[i].action) {
                    case "direct": direct++; break;
                    case "reject": reject++; break;
                    default: proxy++; break;
                }
            }
        }

        private void load_rule_cache () {
            var info = this.get_active_rule_source ();
            if (info == null && this.rule_sources.length > 0) {
                info = this.rule_sources[0];
                this.active_rule_source_id = info.id;
            }
            this.sync_active_rule_source_metadata (info);
            string path = info != null ? this.get_rule_source_path (info) : this.rule_cache_path;
            if (!GLib.FileUtils.test (path, GLib.FileTest.EXISTS)) return;
            var imported = RuleImporter.import_from_file (path);
            if (imported == null) return;
            for (uint i = 0; i < imported.rules.length; i++) {
                this.imported_domain_rules.add (imported.rules[i]);
            }
        }

        private void migrate_legacy_rule_source () {
            if (this.rule_sources.length > 0 || this.imported_domain_rules.length == 0) return;
            var info = new RuleSourceInfo ();
            info.name = this.rule_source_name != "" ? this.rule_source_name : "Imported Configuration";
            info.url = this.rule_source_url;
            info.updated_at = this.rule_source_updated_at;
            info.cache_file = GLib.Path.get_basename (this.rule_cache_path);
            info.default_policy = this.domain_default_policy;
            this.rule_sources.add (info);
            this.active_rule_source_id = info.id;
        }

        private void rebuild_domain_rule_matcher () {
            this.domain_rule_matcher = new DomainRuleMatcher (
                this.get_effective_domain_rules (),
                this.domain_default_policy
            );
        }

        private string[] read_string_array (Json.Object obj, string member_name) {
            var values = new GLib.GenericArray<string> ();
            var array = obj.get_array_member (member_name);
            array.foreach_element ((source, index, node) => {
                values.add (node.get_string ());
            });
            var result = new string[values.length];
            for (uint i = 0; i < values.length; i++) {
                result[i] = values[i];
            }
            return result;
        }

        private void write_string_array (Json.Builder builder, string[] values) {
            builder.begin_array ();
            foreach (var value in values) {
                builder.add_string_value (value);
            }
            builder.end_array ();
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

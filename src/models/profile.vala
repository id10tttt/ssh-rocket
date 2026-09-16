namespace Sshuttle {

    public enum TunnelState {
        DISCONNECTED,
        CONNECTING,
        CONNECTED,
        DISCONNECTING,
        ERROR;

        public string to_string () {
            switch (this) {
                case CONNECTED:
                    return "connected";
                case CONNECTING:
                    return "connecting";
                case DISCONNECTING:
                    return "disconnecting";
                case ERROR:
                    return "error";
                default:
                    return "disconnected";
            }
        }

        public string get_label () {
            switch (this) {
                case CONNECTED:
                    return "Connected";
                case CONNECTING:
                    return "Connecting";
                case DISCONNECTING:
                    return "Disconnecting";
                case ERROR:
                    return "Error";
                default:
                    return "Disconnected";
            }
        }
    }

    public class Profile : Object {
        public string id { get; set; default = ""; }
        public string name { get; set; default = "New Server"; }
        public string host { get; set; default = ""; }
        public int port { get; set; default = 22; }
        public string username { get; set; default = ""; }
        public string auth_type { get; set; default = "key"; } // "key", "password"；"agent" 仅兼容旧配置
        public string key_path { get; set; default = ""; }
        public string password { get; set; default = ""; }
        // 仅用于将旧版 Profile 中的网络配置迁移到全局设置，不再写入 Profile。
        public string[] legacy_routes { get; set; }
        public string[] legacy_exclude { get; set; }
        public bool legacy_dns { get; set; default = true; }
        public bool legacy_ipv6 { get; set; default = false; }
        public string legacy_verbosity { get; set; default = "normal"; }
        public bool legacy_auto_connect { get; set; default = false; }

        public Profile () {
            if (this.id == "") {
                this.id = GLib.Uuid.string_random ();
            }
            this.legacy_routes = new string[] { "0.0.0.0/0" };
            this.legacy_exclude = new string[] {};
        }

        public string get_ssh_target () {
            string target = this.host;
            if (this.username != "") {
                target = @"$(this.username)@$(target)";
            }
            if (this.port != 0 && this.port != 22) {
                target = @"$(target):$(this.port)";
            }
            return target;
        }

        public string get_login_mode_label () {
            if (this.auth_type == "key") {
                if (this.key_path != "") {
                    string basename = GLib.Path.get_basename (this.key_path);
                    return @"Key ($basename)";
                }
                return "Private Key";
            } else if (this.auth_type == "password") {
                return "Password";
            }
            return "SSH Agent";
        }

        public string get_summary () {
            return @"$(this.get_ssh_target ()) · $(this.get_login_mode_label ())";
        }

        public Json.Node serialize () {
            var builder = new Json.Builder ();
            builder.begin_object ();

            builder.set_member_name ("id");
            builder.add_string_value (this.id);

            builder.set_member_name ("name");
            builder.add_string_value (this.name);

            builder.set_member_name ("host");
            builder.add_string_value (this.host);

            builder.set_member_name ("port");
            builder.add_int_value (this.port);

            builder.set_member_name ("username");
            builder.add_string_value (this.username);

            builder.set_member_name ("auth_type");
            builder.add_string_value (this.auth_type);

            builder.set_member_name ("key_path");
            builder.add_string_value (this.key_path);

            builder.set_member_name ("password");
            builder.add_string_value (this.password);

            builder.end_object ();
            return builder.get_root ();
        }

        public static Profile deserialize (Json.Object obj) {
            var p = new Profile ();

            if (obj.has_member ("id")) {
                p.id = obj.get_string_member ("id");
            }
            if (obj.has_member ("name")) {
                p.name = obj.get_string_member ("name");
            }
            if (obj.has_member ("host")) {
                p.host = obj.get_string_member ("host");
            }
            if (obj.has_member ("port")) {
                p.port = (int) obj.get_int_member ("port");
            }
            if (obj.has_member ("username")) {
                p.username = obj.get_string_member ("username");
            }
            if (obj.has_member ("auth_type")) {
                p.auth_type = obj.get_string_member ("auth_type");
            }
            if (obj.has_member ("key_path")) {
                p.key_path = obj.get_string_member ("key_path");
            }
            if (obj.has_member ("password")) {
                p.password = obj.get_string_member ("password");
            }
            if (obj.has_member ("dns")) {
                p.legacy_dns = obj.get_boolean_member ("dns");
            }
            if (obj.has_member ("ipv6")) {
                p.legacy_ipv6 = obj.get_boolean_member ("ipv6");
            }
            if (obj.has_member ("verbosity")) {
                p.legacy_verbosity = obj.get_string_member ("verbosity");
            }
            if (obj.has_member ("auto_connect")) {
                p.legacy_auto_connect = obj.get_boolean_member ("auto_connect");
            }

            if (obj.has_member ("routes")) {
                var arr = obj.get_array_member ("routes");
                var r_list = new GLib.GenericArray<string> ();
                arr.foreach_element ((array, index, element_node) => {
                    r_list.add (element_node.get_string ());
                });
                var r_arr = new string[r_list.length];
                for (uint i = 0; i < r_list.length; i++) {
                    r_arr[i] = r_list[i];
                }
                p.legacy_routes = r_arr;
            }

            if (obj.has_member ("exclude")) {
                var arr = obj.get_array_member ("exclude");
                var exc_list = new GLib.GenericArray<string> ();
                arr.foreach_element ((array, index, element_node) => {
                    exc_list.add (element_node.get_string ());
                });
                var exc_arr = new string[exc_list.length];
                for (uint i = 0; i < exc_list.length; i++) {
                    exc_arr[i] = exc_list[i];
                }
                p.legacy_exclude = exc_arr;
            }

            return p;
        }
    }

    public class NetworkSettings : Object {
        public string[] routes { get; set; }
        public string[] exclude { get; set; }
        public bool dns { get; set; default = true; }
        public bool ipv6 { get; set; default = false; }
        public string verbosity { get; set; default = "normal"; }
        public bool auto_connect { get; set; default = false; }

        public NetworkSettings () {
            this.routes = new string[] { "0.0.0.0/0" };
            this.exclude = new string[] {};
        }
    }
}

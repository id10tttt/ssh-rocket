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
        public string[] routes { get; set; }
        public string[] exclude { get; set; }
        public bool dns { get; set; default = true; }
        public bool ipv6 { get; set; default = false; }
        public string method { get; set; default = "auto"; }
        public string verbosity { get; set; default = "normal"; }
        public bool auto_connect { get; set; default = false; }

        public Profile () {
            if (this.id == "") {
                this.id = GLib.Uuid.string_random ();
            }
            this.routes = new string[] { "0.0.0.0/0" };
            this.exclude = new string[] {};
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

        public string get_summary () {
            var parts = new GLib.GenericArray<string> ();
            if (this.routes.length > 0) {
                parts.add (string.joinv (", ", this.routes));
            }
            if (this.dns) {
                parts.add ("DNS");
            }
            if (this.ipv6) {
                parts.add ("IPv6");
            }
            if (this.exclude.length > 0) {
                parts.add (@"Exclude $(this.exclude.length)");
            }
            if (parts.length == 0) {
                return "No routes";
            }
            var arr = new string[parts.length];
            for (uint i = 0; i < parts.length; i++) {
                arr[i] = parts[i];
            }
            return string.joinv (" · ", arr);
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

            builder.set_member_name ("dns");
            builder.add_boolean_value (this.dns);

            builder.set_member_name ("ipv6");
            builder.add_boolean_value (this.ipv6);

            builder.set_member_name ("method");
            builder.add_string_value (this.method);

            builder.set_member_name ("verbosity");
            builder.add_string_value (this.verbosity);

            builder.set_member_name ("auto_connect");
            builder.add_boolean_value (this.auto_connect);

            builder.set_member_name ("routes");
            builder.begin_array ();
            foreach (var r in this.routes) {
                builder.add_string_value (r);
            }
            builder.end_array ();

            builder.set_member_name ("exclude");
            builder.begin_array ();
            foreach (var exc in this.exclude) {
                builder.add_string_value (exc);
            }
            builder.end_array ();

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
            if (obj.has_member ("dns")) {
                p.dns = obj.get_boolean_member ("dns");
            }
            if (obj.has_member ("ipv6")) {
                p.ipv6 = obj.get_boolean_member ("ipv6");
            }
            if (obj.has_member ("method")) {
                p.method = obj.get_string_member ("method");
            }
            if (obj.has_member ("verbosity")) {
                p.verbosity = obj.get_string_member ("verbosity");
            }
            if (obj.has_member ("auto_connect")) {
                p.auto_connect = obj.get_boolean_member ("auto_connect");
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
                p.routes = r_arr;
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
                p.exclude = exc_arr;
            }

            return p;
        }
    }
}

namespace Sshuttle {

    public class DomainRule : Object {
        public string pattern { get; set; default = ""; }
        public string action { get; set; default = "proxy"; } // "proxy" 或 "direct"

        private GLib.PatternSpec? pattern_spec = null;
        private string root_domain = "";

        public DomainRule (string pattern, string action = "proxy") {
            this.pattern = pattern.strip ().down ();
            this.action = action;
            this.init_matcher ();
        }

        private void init_matcher () {
            if (this.pattern == "") {
                return;
            }

            // 对于 *.example.com，同时匹配 example.com 自身
            if (this.pattern.has_prefix ("*.")) {
                this.root_domain = this.pattern.substring (2);
            } else {
                this.root_domain = this.pattern;
            }

            this.pattern_spec = new GLib.PatternSpec (this.pattern);
        }

        public bool matches (string domain) {
            string d = domain.strip ().down ();
            if (d == "") {
                return false;
            }

            if (this.root_domain != "" && d == this.root_domain) {
                return true;
            }

            if (this.pattern_spec != null && this.pattern_spec.match_string (d)) {
                return true;
            }

            // 支持后缀匹配，例如 pattern 为 .google.com 匹配 mail.google.com
            if (this.pattern.has_prefix (".")) {
                return d.has_suffix (this.pattern) || d == this.pattern.substring (1);
            }

            return false;
        }

        public Json.Node serialize () {
            var builder = new Json.Builder ();
            builder.begin_object ();
            builder.set_member_name ("pattern");
            builder.add_string_value (this.pattern);
            builder.set_member_name ("action");
            builder.add_string_value (this.action);
            builder.end_object ();
            return builder.get_root ();
        }

        public static DomainRule deserialize (Json.Object obj) {
            string p = "";
            string a = "proxy";
            if (obj.has_member ("pattern")) {
                p = obj.get_string_member ("pattern");
            }
            if (obj.has_member ("action")) {
                a = obj.get_string_member ("action");
            }
            return new DomainRule (p, a);
        }
    }
}

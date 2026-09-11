namespace Sshuttle {

    public class OmegaImportResult : Object {
        public GLib.GenericArray<DomainRule> rules { get; set; }
        public string default_policy { get; set; default = "direct"; }

        public OmegaImportResult () {
            this.rules = new GLib.GenericArray<DomainRule> ();
        }
    }

    public class OmegaImporter : Object {

        public static OmegaImportResult? import_from_file (string file_path) {
            try {
                string content;
                GLib.FileUtils.get_contents (file_path, out content);
                return import_from_string (content);
            } catch (GLib.Error e) {
                warning ("Failed to read file %s: %s", file_path, e.message);
                return null;
            }
        }

        public static OmegaImportResult import_from_string (string content) {
            var result = new OmegaImportResult ();
            string trimmed = content.strip ();

            if (trimmed.has_prefix ("{") && trimmed.has_suffix ("}")) {
                parse_json_backup (trimmed, result);
            } else {
                parse_rule_list_text (trimmed, result);
            }

            return result;
        }

        private static void parse_json_backup (string json_text, OmegaImportResult result) {
            try {
                var parser = new Json.Parser ();
                parser.load_from_data (json_text);
                var root = parser.get_root ();
                if (root == null || root.get_node_type () != Json.NodeType.OBJECT) {
                    return;
                }

                var root_obj = root.get_object ();
                root_obj.foreach_member ((obj, member_name, member_node) => {
                    if (member_node.get_node_type () != Json.NodeType.OBJECT) {
                        return;
                    }

                    var profile_obj = member_node.get_object ();
                    if (profile_obj.has_member ("profileType")) {
                        string p_type = profile_obj.get_string_member ("profileType");
                        if (p_type == "SwitchProfile") {
                            if (profile_obj.has_member ("defaultProfileName")) {
                                string def_name = profile_obj.get_string_member ("defaultProfileName").down ();
                                result.default_policy = (def_name == "direct") ? "direct" : "proxy";
                            }

                            if (profile_obj.has_member ("rules")) {
                                var rules_arr = profile_obj.get_array_member ("rules");
                                rules_arr.foreach_element ((array, index, element_node) => {
                                    if (element_node.get_node_type () == Json.NodeType.OBJECT) {
                                        var r_obj = element_node.get_object ();
                                        string p_name = "proxy";
                                        if (r_obj.has_member ("profileName")) {
                                            string n = r_obj.get_string_member ("profileName").down ();
                                            p_name = (n == "direct") ? "direct" : "proxy";
                                        }

                                        if (r_obj.has_member ("condition")) {
                                            var c_obj = r_obj.get_object_member ("condition");
                                            if (c_obj.has_member ("pattern")) {
                                                string pat = c_obj.get_string_member ("pattern").strip ();
                                                if (pat != "") {
                                                    result.rules.add (new DomainRule (pat, p_name));
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                });
            } catch (GLib.Error e) {
                warning ("Failed to parse Zero Omega JSON backup: %s", e.message);
            }
        }

        private static void parse_rule_list_text (string text, OmegaImportResult result) {
            string[] lines = text.split ("\n");
            foreach (var line in lines) {
                string l = line.strip ();
                if (l == "" || l.has_prefix (";") || l.has_prefix ("#") || l.has_prefix ("@") || l == "[SwitchyOmega Conditions]") {
                    continue;
                }

                // 处理白名单/直连语法
                if (l.has_prefix ("!")) {
                    string pat = l.substring (1).strip ();
                    if (pat != "") {
                        result.rules.add (new DomainRule (pat, "direct"));
                    }
                    continue;
                }

                // 处理 AutoProxy 语法
                if (l.has_prefix ("@@||")) {
                    string domain = l.substring (4).strip ();
                    if (domain != "") {
                        result.rules.add (new DomainRule (@"*.$(domain)", "direct"));
                    }
                    continue;
                }

                if (l.has_prefix ("||")) {
                    string domain = l.substring (2).strip ();
                    if (domain != "") {
                        result.rules.add (new DomainRule (@"*.$(domain)", "proxy"));
                    }
                    continue;
                }

                // 处理 +proxy 或 +direct 语法
                if (l.contains ("+")) {
                    string[] parts = l.split ("+");
                    if (parts.length >= 2) {
                        string pat = parts[0].strip ();
                        string act = parts[1].strip ().down ();
                        string action_name = (act == "direct") ? "direct" : "proxy";
                        if (pat != "") {
                            result.rules.add (new DomainRule (pat, action_name));
                        }
                        continue;
                    }
                }

                // 纯域名通配符
                result.rules.add (new DomainRule (l, "proxy"));
            }
        }

        public static string export_to_rule_list (DomainRule[] rules, string default_policy) {
            var sb = new StringBuilder ();
            sb.append ("[SwitchyOmega Conditions]\n");
            sb.append ("@with result\n\n");

            foreach (var r in rules) {
                sb.append (@"$(r.pattern) +$(r.action)\n");
            }

            return sb.str;
        }
    }
}

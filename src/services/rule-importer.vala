namespace Sshuttle {

    public errordomain RuleImportError {
        INVALID_SOURCE,
        DOWNLOAD_FAILED,
        TOO_LARGE,
        NO_RULES
    }

    public class RuleSetReference : Object {
        public string url { get; construct; }
        public string action { get; construct; }

        public RuleSetReference (string url, string action) {
            Object (url: url, action: action);
        }
    }

    public class RuleImportResult : Object {
        public GLib.GenericArray<DomainRule> rules { get; private set; }
        public GLib.GenericArray<RuleSetReference> rule_sets { get; private set; }
        public GLib.GenericArray<string> warnings { get; private set; }
        public string default_policy { get; set; default = "direct"; }
        public uint direct_count { get; private set; default = 0; }
        public uint proxy_count { get; private set; default = 0; }
        public uint reject_count { get; private set; default = 0; }
        public uint ignored_count { get; set; default = 0; }
        private GLib.HashTable<string, bool> known_rules;
        private GLib.HashTable<string, bool> known_warnings;

        public RuleImportResult () {
            this.rules = new GLib.GenericArray<DomainRule> ();
            this.rule_sets = new GLib.GenericArray<RuleSetReference> ();
            this.warnings = new GLib.GenericArray<string> ();
            this.known_rules = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
            this.known_warnings = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
        }

        public void add_warning (string warning_text) {
            if (this.known_warnings.contains (warning_text)) return;
            this.known_warnings.insert (warning_text, true);
            this.warnings.add (warning_text);
        }

        public void add_rule (DomainRule rule) {
            string key = @"$(rule.rule_type)\n$(rule.pattern)";
            if (this.known_rules.contains (key)) {
                return;
            }
            this.known_rules.insert (key, true);
            this.rules.add (rule);
            if (rule.action == "direct") {
                this.direct_count++;
            } else if (rule.action == "reject") {
                this.reject_count++;
            } else {
                this.proxy_count++;
            }
        }

        public string to_cache () {
            var builder = new GLib.StringBuilder ();
            builder.append ("# SSH Rocket normalized rule cache\n[Rule]\n");
            for (uint i = 0; i < this.rules.length; i++) {
                var rule = this.rules[i];
                string type;
                switch (rule.rule_type) {
                    case "domain": type = "DOMAIN"; break;
                    case "domain-suffix": type = "DOMAIN-SUFFIX"; break;
                    case "domain-keyword": type = "DOMAIN-KEYWORD"; break;
                    case "ip-cidr": type = ":" in rule.pattern ? "IP-CIDR6" : "IP-CIDR"; break;
                    default: type = "LEGACY"; break;
                }
                builder.append (@"$(type),$(rule.pattern),$(rule.action)\n");
            }
            builder.append (@"FINAL,$(this.default_policy)\n");
            return builder.str;
        }
    }

    /** 解析 Shadowrocket 配置，并兼容旧的 SwitchyOmega 规则文件。 */
    public class RuleImporter : Object {
        public const string DEFAULT_SOURCE_URL = "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
        private const size_t MAX_DOWNLOAD_SIZE = 16 * 1024 * 1024;
        private const uint MAX_RULE_SETS = 8;

        public static RuleImportResult? import_from_file (string file_path) {
            try {
                string content;
                GLib.FileUtils.get_contents (file_path, out content);
                return import_from_string (content);
            } catch (GLib.Error e) {
                warning ("Failed to read rule file %s: %s", file_path, e.message);
                return null;
            }
        }

        public static RuleImportResult import_from_string (string content) {
            var result = new RuleImportResult ();
            string trimmed = content.strip ();
            if (trimmed.has_prefix ("{") && trimmed.has_suffix ("}")) {
                parse_switchy_omega_json (trimmed, result);
            } else if ("[Rule]" in trimmed || "[General]" in trimmed) {
                parse_shadowrocket (trimmed, result);
            } else {
                parse_switchy_omega_text (trimmed, result);
            }
            return result;
        }

        public static async RuleImportResult import_from_url (
            string url,
            GLib.Cancellable? cancellable = null
        ) throws GLib.Error {
            validate_https_url (url);
            var session = new Soup.Session ();
            session.timeout = 20;
            session.user_agent = "SSH-Rocket/1.0";
            string content = yield download_text (session, url, cancellable);
            var result = import_from_string (content);
            yield import_rule_sets (result, cancellable, session);
            if (result.rules.length == 0) {
                throw new RuleImportError.NO_RULES ("The source contains no supported rules");
            }
            return result;
        }

        public static async void import_rule_sets (
            RuleImportResult result,
            GLib.Cancellable? cancellable = null,
            Soup.Session? existing_session = null
        ) {
            var session = existing_session ?? new Soup.Session ();
            session.timeout = 20;
            session.user_agent = "SSH-Rocket/1.0";
            uint rule_set_count = uint.min (result.rule_sets.length, MAX_RULE_SETS);
            for (uint i = 0; i < rule_set_count; i++) {
                var reference = result.rule_sets[i];
                try {
                    validate_https_url (reference.url);
                    string list_content = yield download_text (session, reference.url, cancellable);
                    parse_rule_set (list_content, reference.action, result);
                } catch (GLib.Error e) {
                    result.ignored_count++;
                    result.add_warning (@"Rule set was skipped: $(e.message)");
                }
            }
            if (result.rule_sets.length > MAX_RULE_SETS) {
                result.ignored_count += result.rule_sets.length - MAX_RULE_SETS;
                result.add_warning ("Additional rule sets were skipped");
            }
        }

        private static void validate_https_url (string url) throws RuleImportError {
            try {
                var uri = GLib.Uri.parse (url, GLib.UriFlags.NONE);
                if (uri.get_scheme () != "https" || uri.get_host () == null || uri.get_host () == "") {
                    throw new RuleImportError.INVALID_SOURCE ("Only HTTPS rule URLs are supported");
                }
            } catch (GLib.UriError e) {
                throw new RuleImportError.INVALID_SOURCE ("Invalid rule URL");
            }
        }

        private static async string download_text (
            Soup.Session session,
            string url,
            GLib.Cancellable? cancellable
        ) throws GLib.Error {
            var message = new Soup.Message ("GET", url);
            var stream = yield session.send_async (message, GLib.Priority.DEFAULT, cancellable);
            if (message.uri.get_scheme () != "https") {
                throw new RuleImportError.INVALID_SOURCE ("Redirected rule URL must use HTTPS");
            }
            if (message.status_code < 200 || message.status_code >= 300) {
                throw new RuleImportError.DOWNLOAD_FAILED (
                    "Download failed with HTTP %u".printf (message.status_code)
                );
            }
            int64 declared_length = message.response_headers.get_content_length ();
            if (declared_length > (int64) MAX_DOWNLOAD_SIZE) {
                throw new RuleImportError.TOO_LARGE ("Rule source exceeds the 16 MB limit");
            }
            var buffer = new GLib.ByteArray ();
            size_t received = 0;
            while (true) {
                var chunk = yield stream.read_bytes_async (64 * 1024, GLib.Priority.DEFAULT, cancellable);
                unowned uint8[] chunk_data = chunk.get_data ();
                if (chunk_data.length == 0) break;
                received += chunk_data.length;
                if (received > MAX_DOWNLOAD_SIZE) {
                    throw new RuleImportError.TOO_LARGE ("Rule source exceeds the 16 MB limit");
                }
                buffer.append (chunk_data);
            }
            var bytes = GLib.ByteArray.free_to_bytes ((owned) buffer);
            unowned uint8[] data = bytes.get_data ();
            uint8[] terminated = new uint8[data.length + 1];
            GLib.Memory.copy (terminated, data, data.length);
            terminated[data.length] = 0;
            return (string) terminated;
        }

        private static void parse_shadowrocket (string text, RuleImportResult result) {
            string section = "";
            foreach (var raw_line in text.split ("\n")) {
                string line = raw_line.strip ();
                if (line == "" || line.has_prefix ("#") || line.has_prefix (";")) {
                    continue;
                }
                if (line.has_prefix ("[") && line.has_suffix ("]")) {
                    section = line.down ();
                    if (section != "[general]" && section != "[rule]") {
                        result.add_warning (@"Unsupported section ignored: $(line)");
                    }
                    continue;
                }
                if (section == "[general]") {
                    parse_general_line (line, result);
                } else if (section == "[rule]") {
                    parse_shadowrocket_rule (line, null, result, true);
                } else if (section != "") {
                    result.ignored_count++;
                }
            }
        }

        private static void parse_general_line (string line, RuleImportResult result) {
            int equals = line.index_of ("=");
            if (equals < 0) {
                return;
            }
            string key = line.substring (0, equals).strip ().down ();
            if (key != "skip-proxy" && key != "bypass-tun") {
                return;
            }
            string values = line.substring (equals + 1);
            foreach (var raw_value in values.split (",")) {
                string value = raw_value.strip ().down ();
                if (value == "" || value == "localhost") {
                    continue;
                }
                if (is_network (value)) {
                    result.add_rule (new DomainRule (value, "direct", "ip-cidr"));
                } else if (value.has_prefix ("*.")) {
                    result.add_rule (new DomainRule (value.substring (2), "direct", "domain-suffix"));
                } else {
                    result.add_rule (new DomainRule (value, "direct", "domain"));
                }
            }
        }

        private static void parse_rule_set (string text, string action, RuleImportResult result) {
            foreach (var raw_line in text.split ("\n")) {
                string line = raw_line.strip ();
                if (line == "" || line.has_prefix ("#") || line.has_prefix (";")) {
                    continue;
                }
                parse_shadowrocket_rule (line, action, result, false);
            }
        }

        private static void parse_shadowrocket_rule (
            string line,
            string? inherited_action,
            RuleImportResult result,
            bool collect_rule_sets
        ) {
            string[] parts = line.split (",");
            if (parts.length < 2) {
                result.ignored_count++;
                return;
            }
            string type = parts[0].strip ().up ();
            string value = parts[1].strip ().down ();
            string action = inherited_action ?? (parts.length >= 3 ? parts[2] : "proxy");
            action = normalize_action (action);

            switch (type) {
                case "DOMAIN":
                    result.add_rule (new DomainRule (value, action, "domain"));
                    break;
                case "DOMAIN-SUFFIX":
                    result.add_rule (new DomainRule (value, action, "domain-suffix"));
                    break;
                case "DOMAIN-KEYWORD":
                    result.add_rule (new DomainRule (value, action, "domain-keyword"));
                    break;
                case "IP-CIDR":
                case "IP-CIDR6":
                    result.add_rule (new DomainRule (value, action, "ip-cidr"));
                    break;
                case "LEGACY":
                    result.add_rule (new DomainRule (value, action));
                    break;
                case "RULE-SET":
                    if (collect_rule_sets && value.has_prefix ("https://")) {
                        result.rule_sets.add (new RuleSetReference (parts[1].strip (), action));
                    } else {
                        result.ignored_count++;
                    }
                    break;
                case "FINAL":
                    result.default_policy = normalize_action (parts[1]) == "proxy" ? "proxy" : "direct";
                    break;
                default:
                    result.ignored_count++;
                    break;
            }
        }

        private static string normalize_action (string value) {
            string action = value.strip ().down ();
            if (action.has_prefix ("reject")) {
                return "reject";
            }
            if (action == "direct") {
                return "direct";
            }
            return "proxy";
        }

        private static bool is_network (string value) {
            string address = value.split ("/", 2)[0];
            return new GLib.InetAddress.from_string (address) != null;
        }

        private static void parse_switchy_omega_text (string text, RuleImportResult result) {
            foreach (var raw_line in text.split ("\n")) {
                string line = raw_line.strip ();
                if (line == "" || line.has_prefix (";") || line.has_prefix ("#") ||
                    line.has_prefix ("@") || line == "[SwitchyOmega Conditions]") {
                    continue;
                }
                if (line.has_prefix ("!")) {
                    result.add_rule (new DomainRule (line.substring (1).strip (), "direct"));
                } else if (line.has_prefix ("@@||")) {
                    string domain = line.substring (4).replace ("^", "").strip ();
                    if (domain != "") {
                        result.add_rule (new DomainRule (domain, "direct", "domain-suffix"));
                    }
                } else if (line.has_prefix ("||")) {
                    string domain = line.substring (2).replace ("^", "").strip ();
                    if (domain != "") {
                        result.add_rule (new DomainRule (domain, "proxy", "domain-suffix"));
                    }
                } else if (line.contains ("+")) {
                    string[] parts = line.split ("+");
                    if (parts.length >= 2 && parts[0].strip () != "") {
                        result.add_rule (new DomainRule (parts[0], normalize_action (parts[1])));
                    }
                } else {
                    result.add_rule (new DomainRule (line, "proxy"));
                }
            }
        }

        private static void parse_switchy_omega_json (string json_text, RuleImportResult result) {
            try {
                var parser = new Json.Parser ();
                parser.load_from_data (json_text);
                var root = parser.get_root ();
                if (root == null || root.get_node_type () != Json.NodeType.OBJECT) {
                    return;
                }
                root.get_object ().foreach_member ((obj, member_name, member_node) => {
                    if (member_node.get_node_type () != Json.NodeType.OBJECT) return;
                    var profile = member_node.get_object ();
                    if (!profile.has_member ("profileType") ||
                        profile.get_string_member ("profileType") != "SwitchProfile") return;
                    if (profile.has_member ("defaultProfileName")) {
                        result.default_policy = normalize_action (profile.get_string_member ("defaultProfileName"));
                    }
                    if (!profile.has_member ("rules")) return;
                    profile.get_array_member ("rules").foreach_element ((array, index, node) => {
                        if (node.get_node_type () != Json.NodeType.OBJECT) return;
                        var item = node.get_object ();
                        if (!item.has_member ("condition")) return;
                        var condition = item.get_object_member ("condition");
                        if (!condition.has_member ("pattern")) return;
                        string action = item.has_member ("profileName")
                            ? normalize_action (item.get_string_member ("profileName"))
                            : "proxy";
                        result.add_rule (new DomainRule (condition.get_string_member ("pattern"), action));
                    });
                });
            } catch (GLib.Error e) {
                warning ("Failed to parse SwitchyOmega backup: %s", e.message);
            }
        }
    }
}

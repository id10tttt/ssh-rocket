namespace Sshuttle {

    public class DomainRule : Object {
        public string pattern { get; set; default = ""; }
        public string action { get; set; default = "proxy"; }
        public string rule_type { get; set; default = "legacy"; }

        private GLib.PatternSpec? pattern_spec = null;
        private string root_domain = "";

        public DomainRule (string pattern, string action = "proxy", string rule_type = "legacy") {
            this.pattern = pattern.strip ().down ();
            string normalized_action = action.strip ().down ();
            this.action = normalized_action == "direct" || normalized_action == "reject"
                ? normalized_action
                : "proxy";
            this.rule_type = rule_type.strip ().down ();
            this.init_matcher ();
        }

        private void init_matcher () {
            if (this.pattern == "" || this.rule_type != "legacy") {
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

            if (this.rule_type == "domain") {
                return d == this.pattern;
            }
            if (this.rule_type == "domain-suffix") {
                return d == this.pattern || d.has_suffix (@".$(this.pattern)");
            }
            if (this.rule_type == "domain-keyword") {
                return this.pattern in d;
            }
            if (this.rule_type == "ip-cidr") {
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
            builder.set_member_name ("rule_type");
            builder.add_string_value (this.rule_type);
            builder.end_object ();
            return builder.get_root ();
        }

        public static DomainRule deserialize (Json.Object obj) {
            string p = "";
            string a = "proxy";
            string t = "legacy";
            if (obj.has_member ("pattern")) {
                p = obj.get_string_member ("pattern");
            }
            if (obj.has_member ("action")) {
                a = obj.get_string_member ("action");
            }
            if (obj.has_member ("rule_type")) {
                t = obj.get_string_member ("rule_type");
            }
            return new DomainRule (p, a, t);
        }
    }

    internal class IndexedDomainRule : Object {
        public DomainRule rule { get; construct; }
        public uint priority { get; construct; }

        public IndexedDomainRule (DomainRule rule, uint priority) {
            Object (rule: rule, priority: priority);
        }
    }

    /** 为大型域名规则集建立精确与后缀索引，同时保留原始优先级。 */
    public class DomainRuleMatcher : Object {
        private GLib.HashTable<string, IndexedDomainRule> exact_rules;
        private GLib.HashTable<string, IndexedDomainRule> suffix_rules;
        private GLib.GenericArray<IndexedDomainRule> fallback_rules;
        private string default_policy;

        public DomainRuleMatcher (DomainRule[] rules, string default_policy) {
            this.exact_rules = new GLib.HashTable<string, IndexedDomainRule> (GLib.str_hash, GLib.str_equal);
            this.suffix_rules = new GLib.HashTable<string, IndexedDomainRule> (GLib.str_hash, GLib.str_equal);
            this.fallback_rules = new GLib.GenericArray<IndexedDomainRule> ();
            this.default_policy = default_policy == "proxy" ? "proxy" : "direct";

            for (uint i = 0; i < rules.length; i++) {
                var indexed = new IndexedDomainRule (rules[i], i);
                if (rules[i].rule_type == "domain") {
                    if (!this.exact_rules.contains (rules[i].pattern)) {
                        this.exact_rules.insert (rules[i].pattern, indexed);
                    }
                } else if (rules[i].rule_type == "domain-suffix") {
                    if (!this.suffix_rules.contains (rules[i].pattern)) {
                        this.suffix_rules.insert (rules[i].pattern, indexed);
                    }
                } else if (rules[i].rule_type != "ip-cidr") {
                    this.fallback_rules.add (indexed);
                }
            }
        }

        public void set_default_policy (string policy) {
            this.default_policy = policy == "proxy" ? "proxy" : "direct";
        }

        public string resolve (string? domain, out bool matched = null) {
            matched = false;
            if (domain == null || domain.strip () == "") {
                return this.default_policy;
            }

            string normalized = domain.strip ().down ();
            IndexedDomainRule? best = this.exact_rules.lookup (normalized);
            string suffix = normalized;
            while (suffix != "") {
                var candidate = this.suffix_rules.lookup (suffix);
                if (candidate != null && (best == null || candidate.priority < best.priority)) {
                    best = candidate;
                }
                int dot = suffix.index_of (".");
                if (dot < 0 || dot + 1 >= suffix.length) {
                    break;
                }
                suffix = suffix.substring (dot + 1);
            }

            for (uint i = 0; i < this.fallback_rules.length; i++) {
                var candidate = this.fallback_rules[i];
                if (best != null && candidate.priority >= best.priority) {
                    continue;
                }
                if (candidate.rule.matches (normalized)) {
                    best = candidate;
                }
            }

            if (best != null) {
                matched = true;
                return best.rule.action;
            }
            return this.default_policy;
        }
    }
}

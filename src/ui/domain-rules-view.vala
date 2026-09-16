namespace Sshuttle {

    public class DomainRulesView : Adw.PreferencesGroup {
        private const uint RULE_BATCH_SIZE = 20;
        private ConfigManager config_manager;
        private Adw.ComboRow source_selector_row;
        private Adw.ActionRow source_row;
        private Gtk.Button import_source_button;
        private Gtk.Button update_source_button;
        private Gtk.Button remove_source_button;
        private Gtk.Spinner source_spinner;
        private Adw.ComboRow default_policy_row;
        private Adw.EntryRow new_pattern_row;
        private Adw.ComboRow rule_type_row;
        private Gtk.DropDown action_dropdown;
        private Adw.EntryRow search_row;
        private Gtk.Box rules_list_box;
        private Gtk.Button load_more_button;
        private Gtk.Button clear_custom_button;
        private GLib.GenericArray<DomainRule> filtered_rules;
        private uint filtered_custom_count = 0;
        private uint loaded_rule_count = 0;
        private bool scroll_connected = false;
        private bool refreshing_sources = false;

        public DomainRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.filtered_rules = new GLib.GenericArray<DomainRule> ();
            this.title = "Domain and IP Routing";
            this.description = "Import Shadowrocket rules and add custom overrides.";

            this.source_selector_row = new Adw.ComboRow ();
            this.source_selector_row.title = "Local Configurations";
            this.source_selector_row.notify["selected"].connect (this.on_source_selected);
            this.add (this.source_selector_row);

            this.source_row = new Adw.ActionRow ();
            this.source_row.add_prefix (new Gtk.Image.from_icon_name ("folder-download-symbolic"));
            this.source_spinner = new Gtk.Spinner ();
            this.source_spinner.valign = Gtk.Align.CENTER;
            this.source_spinner.visible = false;
            this.source_row.add_suffix (this.source_spinner);

            this.update_source_button = new Gtk.Button.from_icon_name ("view-refresh-symbolic");
            this.update_source_button.tooltip_text = "Update rule source";
            this.update_source_button.valign = Gtk.Align.CENTER;
            this.update_source_button.add_css_class ("flat");
            this.update_source_button.clicked.connect (this.on_update_source_clicked);
            this.source_row.add_suffix (this.update_source_button);

            this.import_source_button = new Gtk.Button.with_label ("Import…");
            this.import_source_button.valign = Gtk.Align.CENTER;
            this.import_source_button.add_css_class ("suggested-action");
            this.import_source_button.clicked.connect (this.show_import_dialog);
            this.source_row.add_suffix (this.import_source_button);

            this.remove_source_button = new Gtk.Button.from_icon_name ("user-trash-symbolic");
            this.remove_source_button.tooltip_text = "Remove imported source";
            this.remove_source_button.valign = Gtk.Align.CENTER;
            this.remove_source_button.add_css_class ("flat");
            this.remove_source_button.clicked.connect (this.confirm_remove_source);
            this.source_row.add_suffix (this.remove_source_button);
            this.add (this.source_row);

            this.default_policy_row = new Adw.ComboRow ();
            this.default_policy_row.title = "Default Policy";
            this.default_policy_row.subtitle = "Used when neither a rule nor an app selection matches";
            this.default_policy_row.model = Native.string_list ({ "direct", "proxy" });
            this.default_policy_row.selected = this.config_manager.get_domain_default_policy () == "proxy" ? 1 : 0;
            this.default_policy_row.notify["selected"].connect (() => {
                if (this.refreshing_sources) return;
                string policy = this.default_policy_row.selected == 1 ? "proxy" : "direct";
                this.config_manager.set_domain_default_policy (policy);
            });
            this.add (this.default_policy_row);

            var custom_header = new Adw.ActionRow ();
            custom_header.title = "Custom Overrides";
            custom_header.subtitle = "Custom rules take priority over the imported source";
            this.clear_custom_button = new Gtk.Button.with_label ("Clear");
            this.clear_custom_button.valign = Gtk.Align.CENTER;
            this.clear_custom_button.add_css_class ("destructive-action");
            this.clear_custom_button.clicked.connect (this.confirm_clear_custom_rules);
            custom_header.add_suffix (this.clear_custom_button);
            this.add (custom_header);

            this.new_pattern_row = new Adw.EntryRow ();
            this.new_pattern_row.title = "Domain, IP, or CIDR";
            this.rule_type_row = new Adw.ComboRow ();
            this.rule_type_row.title = "Rule Type";
            this.rule_type_row.model = Native.string_list ({
                "DOMAIN-SUFFIX", "DOMAIN", "DOMAIN-KEYWORD", "IP-CIDR"
            });
            this.rule_type_row.selected = 0;
            this.add (this.rule_type_row);
            this.action_dropdown = new Gtk.DropDown (
                Native.string_list ({ "direct", "proxy", "reject" }), null
            );
            this.action_dropdown.selected = 1;
            this.action_dropdown.valign = Gtk.Align.CENTER;
            this.new_pattern_row.add_suffix (this.action_dropdown);
            var add_button = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_button.tooltip_text = "Add custom rule";
            add_button.valign = Gtk.Align.CENTER;
            add_button.add_css_class ("suggested-action");
            add_button.clicked.connect (this.on_add_rule_clicked);
            this.new_pattern_row.add_suffix (add_button);
            this.new_pattern_row.entry_activated.connect (this.on_add_rule_clicked);
            this.add (this.new_pattern_row);

            this.search_row = new Adw.EntryRow ();
            this.search_row.title = "Search Rules";
            this.search_row.notify["text"].connect (this.filter_rules);
            this.add (this.search_row);
            this.rules_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            this.rules_list_box.margin_top = 8;
            this.add (this.rules_list_box);

            this.load_more_button = new Gtk.Button.with_label ("Load More");
            this.load_more_button.margin_top = 8;
            this.load_more_button.halign = Gtk.Align.CENTER;
            this.load_more_button.clicked.connect (this.append_next_rule_batch);
            this.add (this.load_more_button);

            this.map.connect (this.connect_scroll_loader);

            this.config_manager.domain_rules_changed.connect (() => {
                this.refresh_source_row ();
                this.refresh_rules_list ();
            });
            this.refresh_source_row ();
            this.refresh_rules_list ();
        }

        private void refresh_source_row () {
            var sources = this.config_manager.get_rule_sources ();
            var names = new string[sources.length];
            uint selected = 0;
            string active_id = this.config_manager.get_active_rule_source_id ();
            for (uint i = 0; i < sources.length; i++) {
                names[i] = sources[i].name;
                if (sources[i].id == active_id) selected = i;
            }
            this.refreshing_sources = true;
            this.source_selector_row.model = Native.string_list (names);
            this.source_selector_row.selected = selected;
            this.source_selector_row.subtitle = "%u configurations".printf (sources.length);
            this.source_selector_row.visible = sources.length > 0;
            this.default_policy_row.selected = this.config_manager.get_domain_default_policy () == "proxy" ? 1 : 0;
            this.refreshing_sources = false;

            uint direct;
            uint proxy;
            uint reject;
            this.config_manager.get_imported_rule_counts (out direct, out proxy, out reject);
            bool has_source = this.config_manager.get_imported_rule_count () > 0;
            this.source_row.title = has_source
                ? (this.config_manager.get_rule_source_name () != ""
                    ? this.config_manager.get_rule_source_name () : "Imported Rule Source")
                : "No Imported Rule Source";
            if (has_source) {
                string summary = "%u direct · %u proxy · %u reject".printf (direct, proxy, reject);
                int64 updated_at = this.config_manager.get_rule_source_updated_at ();
                if (updated_at > 0) {
                    var updated = new GLib.DateTime.from_unix_local (updated_at);
                    summary += " · " + updated.format ("%Y-%m-%d %H:%M");
                }
                this.source_row.subtitle = summary;
            } else {
                this.source_row.subtitle = "Import a Shadowrocket configuration from URL or file";
            }
            this.update_source_button.visible = this.config_manager.get_rule_source_url () != "";
            this.remove_source_button.visible = has_source;
        }

        private void on_source_selected () {
            if (this.refreshing_sources) return;
            var sources = this.config_manager.get_rule_sources ();
            uint selected = this.source_selector_row.selected;
            if (selected < sources.length) {
                this.config_manager.set_active_rule_source (sources[selected].id);
            }
        }

        private void set_source_busy (bool busy) {
            this.source_spinner.visible = busy;
            this.source_spinner.spinning = busy;
            this.update_source_button.sensitive = !busy;
            this.import_source_button.sensitive = !busy;
            this.remove_source_button.sensitive = !busy;
            if (busy) this.source_row.subtitle = "Downloading and parsing rules…";
        }

        private void show_import_dialog () {
            var dialog = new Adw.AlertDialog ("Import Rule Source", null);
            var group = new Adw.PreferencesGroup ();
            var url_row = new Adw.EntryRow ();
            url_row.title = "HTTPS URL";
            string current_url = this.config_manager.get_rule_source_url ();
            url_row.text = current_url != "" ? current_url : RuleImporter.DEFAULT_SOURCE_URL;
            group.add (url_row);
            dialog.set_extra_child (group);
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("file", "Choose File…");
            dialog.add_response ("import", "Import");
            dialog.set_response_appearance ("import", Adw.ResponseAppearance.SUGGESTED);
            dialog.response.connect ((response) => {
                if (response == "import") this.import_url.begin (url_row.text.strip (), false);
                else if (response == "file") this.choose_rule_file ();
            });
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void on_update_source_clicked () {
            string url = this.config_manager.get_rule_source_url ();
            if (url != "") this.import_url.begin (url, true);
        }

        private async void import_url (string url, bool replace_active) {
            if (url == "") return;
            this.set_source_busy (true);
            try {
                var imported = yield RuleImporter.import_from_url (url);
                if (replace_active) {
                    this.config_manager.set_imported_rule_source (
                        imported, url, this.config_manager.get_rule_source_name ()
                    );
                } else {
                    var uri = GLib.Uri.parse (url, GLib.UriFlags.NONE);
                    string name = GLib.Path.get_basename (uri.get_path ());
                    this.config_manager.add_imported_rule_source (
                        imported, url, name != "" ? name : "Shadowrocket Rule Source"
                    );
                }
                this.default_policy_row.selected = imported.default_policy == "proxy" ? 1 : 0;
                this.show_import_result (imported);
            } catch (GLib.Error e) {
                this.show_message ("Import Failed", e.message);
            } finally {
                this.set_source_busy (false);
                this.refresh_source_row ();
            }
        }

        private void choose_rule_file () {
            var dialog = new Gtk.FileDialog ();
            dialog.title = "Import Rule Config";
            var filter = new Gtk.FileFilter ();
            filter.name = "Rule Config (*.conf, *.txt, *.json, *.bak)";
            string[] patterns = { "*.conf", "*.txt", "*.json", "*.bak" };
            foreach (var pattern in patterns) filter.add_pattern (pattern);
            var filters = new GLib.ListStore (typeof (Gtk.FileFilter));
            filters.append (filter);
            dialog.filters = filters;
            dialog.open.begin (this.get_root () as Gtk.Window, null, (obj, result) => {
                try {
                    var file = dialog.open.end (result);
                    string? path = file.get_path ();
                    if (path == null) return;
                    this.import_file.begin (path, file.get_basename () ?? "Imported Rule Source");
                } catch (GLib.Error e) {
                    if (!(e is GLib.IOError.CANCELLED)) this.show_message ("Import Failed", e.message);
                }
            });
        }

        private async void import_file (string path, string source_name) {
            this.set_source_busy (true);
            try {
                var imported = RuleImporter.import_from_file (path);
                if (imported == null || imported.rules.length == 0) {
                    this.show_message ("Import Failed", "The file contains no supported rules.");
                    return;
                }
                yield RuleImporter.import_rule_sets (imported);
                this.config_manager.add_imported_rule_source (imported, "", source_name);
                this.default_policy_row.selected = imported.default_policy == "proxy" ? 1 : 0;
                this.show_import_result (imported);
            } catch (GLib.Error e) {
                this.show_message ("Import Failed", e.message);
            } finally {
                this.set_source_busy (false);
                this.refresh_source_row ();
            }
        }

        private void show_import_result (RuleImportResult result) {
            string message = "%u direct · %u proxy · %u reject".printf (
                result.direct_count, result.proxy_count, result.reject_count
            );
            if (result.ignored_count > 0) {
                message += "\n%u unsupported entries were ignored.".printf (result.ignored_count);
            }
            uint warning_count = uint.min (result.warnings.length, 3);
            for (uint i = 0; i < warning_count; i++) message += "\n" + result.warnings[i];
            this.show_message ("Rules Imported", message);
        }

        private void show_message (string title, string message) {
            var dialog = new Adw.AlertDialog (title, message);
            dialog.add_response ("close", "Close");
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void confirm_remove_source () {
            var dialog = new Adw.AlertDialog ("Remove Imported Source?", "Custom overrides will be kept.");
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("remove", "Remove");
            dialog.set_response_appearance ("remove", Adw.ResponseAppearance.DESTRUCTIVE);
            dialog.response.connect ((response) => {
                if (response == "remove") this.config_manager.clear_imported_rule_source ();
            });
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void confirm_clear_custom_rules () {
            var dialog = new Adw.AlertDialog ("Clear Custom Rules?", null);
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("clear", "Clear");
            dialog.set_response_appearance ("clear", Adw.ResponseAppearance.DESTRUCTIVE);
            dialog.response.connect ((response) => {
                if (response == "clear") this.config_manager.clear_domain_rules ();
            });
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void on_add_rule_clicked () {
            string pattern = this.new_pattern_row.text.strip ();
            if (pattern == "") return;
            string[] actions = { "direct", "proxy", "reject" };
            string[] rule_types = { "domain-suffix", "domain", "domain-keyword", "ip-cidr" };
            this.config_manager.add_domain_rule (
                pattern,
                actions[this.action_dropdown.selected],
                rule_types[this.rule_type_row.selected]
            );
            this.new_pattern_row.text = "";
        }

        private void filter_rules () {
            string query = this.search_row.text.strip ().down ();
            this.filtered_rules.remove_range (0, this.filtered_rules.length);
            this.filtered_custom_count = 0;

            foreach (var rule in this.config_manager.get_domain_rules ()) {
                if (this.rule_matches_query (rule, query)) {
                    this.filtered_rules.add (rule);
                    this.filtered_custom_count++;
                }
            }
            foreach (var rule in this.config_manager.get_imported_domain_rules ()) {
                if (this.rule_matches_query (rule, query)) {
                    this.filtered_rules.add (rule);
                }
            }

            this.clear_rendered_rules ();
            this.append_next_rule_batch ();
        }

        private bool rule_matches_query (DomainRule rule, string query) {
            return query == "" || query in rule.pattern || query in rule.action ||
                query in rule.rule_type;
        }

        /** 每次只创建二十条规则行，滚动到底部后继续追加。 */
        private void append_next_rule_batch () {
            uint end = uint.min (this.loaded_rule_count + RULE_BATCH_SIZE, this.filtered_rules.length);
            while (this.loaded_rule_count < end) {
                this.append_rule_row (
                    this.filtered_rules[this.loaded_rule_count],
                    this.loaded_rule_count < this.filtered_custom_count
                );
                this.loaded_rule_count++;
            }
            this.load_more_button.visible = this.loaded_rule_count < this.filtered_rules.length;
        }

        private void append_rule_row (DomainRule rule, bool custom) {
            var row = new Adw.ActionRow ();
            row.title = GLib.Markup.escape_text (rule.pattern);
            row.subtitle = @"$(rule.rule_type.up ()) · $(rule.action.up ()) · $(custom ? "Custom" : "Imported")";
            row.add_prefix (new Gtk.Image.from_icon_name (
                rule.action == "proxy" ? "ssh-rocket-symbolic" :
                (rule.action == "reject" ? "network-offline-symbolic" : "network-wired-symbolic")
            ));
            if (custom) {
                var edit_button = new Gtk.Button.from_icon_name ("document-edit-symbolic");
                edit_button.tooltip_text = "Edit custom rule";
                edit_button.valign = Gtk.Align.CENTER;
                edit_button.add_css_class ("flat");
                edit_button.clicked.connect (() => this.show_edit_dialog (rule));
                row.add_suffix (edit_button);

                var delete_button = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                delete_button.tooltip_text = "Delete custom rule";
                delete_button.valign = Gtk.Align.CENTER;
                delete_button.add_css_class ("flat");
                string pattern = rule.pattern;
                delete_button.clicked.connect (() => this.config_manager.remove_domain_rule (pattern));
                row.add_suffix (delete_button);
            }
            this.rules_list_box.append (row);
        }

        private void clear_rendered_rules () {
            Gtk.Widget? child = this.rules_list_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.rules_list_box.remove (child);
                child = next;
            }
            this.loaded_rule_count = 0;
        }

        private void connect_scroll_loader () {
            if (this.scroll_connected) return;
            Gtk.Widget? parent = this.get_parent ();
            while (parent != null && !(parent is Gtk.ScrolledWindow)) {
                parent = parent.get_parent ();
            }
            var scrolled = parent as Gtk.ScrolledWindow;
            if (scrolled == null) return;
            this.scroll_connected = true;
            scrolled.vadjustment.value_changed.connect (() => {
                var adjustment = scrolled.vadjustment;
                if (adjustment.value + adjustment.page_size >= adjustment.upper - 160) {
                    this.append_next_rule_batch ();
                }
            });
        }

        private void show_edit_dialog (DomainRule rule) {
            var dialog = new Adw.AlertDialog ("Edit Custom Rule", null);
            var group = new Adw.PreferencesGroup ();
            var pattern_row = new Adw.EntryRow ();
            pattern_row.title = "Pattern";
            pattern_row.text = rule.pattern;
            group.add (pattern_row);
            var action_row = new Adw.ComboRow ();
            action_row.title = "Action";
            action_row.model = Native.string_list ({ "direct", "proxy", "reject" });
            action_row.selected = rule.action == "proxy" ? 1 : (rule.action == "reject" ? 2 : 0);
            group.add (action_row);
            var type_row = new Adw.ComboRow ();
            type_row.title = "Rule Type";
            type_row.model = Native.string_list ({
                "DOMAIN-SUFFIX", "DOMAIN", "DOMAIN-KEYWORD", "IP-CIDR"
            });
            type_row.selected = rule.rule_type == "domain" ? 1 :
                (rule.rule_type == "domain-keyword" ? 2 :
                (rule.rule_type == "ip-cidr" ? 3 : 0));
            group.add (type_row);
            dialog.set_extra_child (group);
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("save", "Save");
            dialog.set_response_appearance ("save", Adw.ResponseAppearance.SUGGESTED);
            dialog.response.connect ((response) => {
                if (response == "save" && pattern_row.text.strip () != "") {
                    string[] actions = { "direct", "proxy", "reject" };
                    string[] rule_types = { "domain-suffix", "domain", "domain-keyword", "ip-cidr" };
                    this.config_manager.update_domain_rule (
                        rule.pattern,
                        pattern_row.text,
                        actions[action_row.selected],
                        rule_types[type_row.selected]
                    );
                }
            });
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void refresh_rules_list () {
            var rules = this.config_manager.get_domain_rules ();
            this.clear_custom_button.visible = rules.length > 0;
            this.search_row.visible = rules.length > 0 || this.config_manager.get_imported_rule_count () > 0;
            this.filter_rules ();
        }
    }
}

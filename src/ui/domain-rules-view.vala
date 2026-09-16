namespace Sshuttle {

    /** 将配置、配置明细和规则列表拆分为独立层级。 */
    public class DomainRulesView : Gtk.Box {
        private const uint RULE_BATCH_SIZE = 20;
        private ConfigManager config_manager;
        private Gtk.Stack page_stack;
        private Adw.PreferencesGroup configurations_group;
        private GLib.GenericArray<Gtk.Widget> configuration_rows;
        private Adw.ActionRow custom_summary_row;
        private Gtk.Label detail_title;
        private Adw.ComboRow default_policy_row;
        private Adw.ActionRow source_row;
        private Adw.ActionRow imported_summary_row;
        private Gtk.Button update_source_button;
        private Gtk.Button remove_source_button;
        private Gtk.Spinner source_spinner;
        private Adw.EntryRow imported_search_row;
        private Adw.PreferencesGroup imported_rules_group;
        private Gtk.Button imported_load_more_button;
        private GLib.GenericArray<Gtk.Widget> imported_rule_rows;
        private GLib.GenericArray<DomainRule> filtered_imported_rules;
        private uint imported_loaded_count = 0;
        private Adw.EntryRow custom_search_row;
        private Adw.PreferencesGroup custom_rules_group;
        private Gtk.Button custom_load_more_button;
        private Gtk.Button clear_custom_button;
        private GLib.GenericArray<Gtk.Widget> custom_rule_rows;
        private GLib.GenericArray<DomainRule> filtered_custom_rules;
        private uint custom_loaded_count = 0;
        private bool refreshing = false;

        public DomainRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.config_manager = config_manager;
            this.configuration_rows = new GLib.GenericArray<Gtk.Widget> ();
            this.imported_rule_rows = new GLib.GenericArray<Gtk.Widget> ();
            this.custom_rule_rows = new GLib.GenericArray<Gtk.Widget> ();
            this.filtered_imported_rules = new GLib.GenericArray<DomainRule> ();
            this.filtered_custom_rules = new GLib.GenericArray<DomainRule> ();

            this.page_stack = new Gtk.Stack ();
            this.page_stack.transition_type = Gtk.StackTransitionType.SLIDE_LEFT_RIGHT;
            this.page_stack.vexpand = true;
            this.append (this.page_stack);

            this.page_stack.add_named (this.build_overview_page (), "overview");
            this.page_stack.add_named (this.build_detail_page (), "detail");
            this.page_stack.add_named (this.build_imported_rules_page (), "imported");
            this.page_stack.add_named (this.build_custom_rules_page (), "custom");
            this.page_stack.visible_child_name = "overview";

            this.config_manager.domain_rules_changed.connect (() => {
                this.refresh_overview ();
                this.refresh_detail ();
                this.refresh_imported_rules ();
                this.refresh_custom_rules ();
            });
            this.config_manager.domain_default_policy_changed.connect (this.refresh_default_policy);
            this.refresh_overview ();
            this.refresh_detail ();
            this.refresh_imported_rules ();
            this.refresh_custom_rules ();
        }

        private Gtk.Widget build_overview_page () {
            var page = new Adw.PreferencesPage ();
            var policy_group = new Adw.PreferencesGroup ();
            policy_group.title = "Default Policy";
            this.default_policy_row = new Adw.ComboRow ();
            this.default_policy_row.title = "Unmatched Traffic";
            this.default_policy_row.model = Native.string_list ({ "DIRECT", "PROXY" });
            this.default_policy_row.notify["selected"].connect (() => {
                if (!this.refreshing) {
                    this.config_manager.set_domain_default_policy (
                        this.default_policy_row.selected == 1 ? "proxy" : "direct"
                    );
                }
            });
            policy_group.add (this.default_policy_row);
            page.add (policy_group);

            this.configurations_group = new Adw.PreferencesGroup ();
            this.configurations_group.title = "Configurations";
            var import_button = new Gtk.Button.with_label ("Import…");
            import_button.valign = Gtk.Align.CENTER;
            import_button.add_css_class ("suggested-action");
            import_button.clicked.connect (this.show_import_dialog);
            this.configurations_group.header_suffix = import_button;
            page.add (this.configurations_group);

            var custom_group = new Adw.PreferencesGroup ();
            custom_group.title = "Custom Rules";
            this.custom_summary_row = this.create_navigation_row (
                "Custom Overrides", "0 rules", "document-edit-symbolic"
            );
            this.custom_summary_row.activated.connect (() => {
                this.page_stack.visible_child_name = "custom";
            });
            custom_group.add (this.custom_summary_row);
            page.add (custom_group);
            return this.wrap_scrolled (page);
        }

        private Gtk.Widget build_detail_page () {
            var page = new Adw.PreferencesPage ();
            var source_group = new Adw.PreferencesGroup ();
            source_group.title = "Source";
            this.source_row = new Adw.ActionRow ();
            this.source_row.add_prefix (new Gtk.Image.from_icon_name ("folder-download-symbolic"));
            this.source_spinner = new Gtk.Spinner ();
            this.source_spinner.valign = Gtk.Align.CENTER;
            this.source_spinner.visible = false;
            this.source_row.add_suffix (this.source_spinner);
            this.update_source_button = new Gtk.Button.from_icon_name ("view-refresh-symbolic");
            this.update_source_button.tooltip_text = "Update Configuration";
            this.update_source_button.valign = Gtk.Align.CENTER;
            this.update_source_button.add_css_class ("flat");
            this.update_source_button.clicked.connect (this.on_update_source_clicked);
            this.source_row.add_suffix (this.update_source_button);
            this.remove_source_button = new Gtk.Button.from_icon_name ("user-trash-symbolic");
            this.remove_source_button.tooltip_text = "Remove Configuration";
            this.remove_source_button.valign = Gtk.Align.CENTER;
            this.remove_source_button.add_css_class ("flat");
            this.remove_source_button.clicked.connect (this.confirm_remove_source);
            this.source_row.add_suffix (this.remove_source_button);
            source_group.add (this.source_row);
            page.add (source_group);

            var rules_group = new Adw.PreferencesGroup ();
            rules_group.title = "Contents";
            this.imported_summary_row = this.create_navigation_row (
                "Rules", "0 rules", "view-list-symbolic"
            );
            this.imported_summary_row.activated.connect (() => {
                this.page_stack.visible_child_name = "imported";
            });
            rules_group.add (this.imported_summary_row);
            page.add (rules_group);
            var content = this.wrap_scrolled (page);
            return this.with_subpage_header (content, "Configuration", "overview", out this.detail_title);
        }

        private Gtk.Widget build_imported_rules_page () {
            var body = new Gtk.Box (Gtk.Orientation.VERTICAL, 12);
            this.prepare_list_body (body);
            var search_group = new Adw.PreferencesGroup ();
            this.imported_search_row = new Adw.EntryRow ();
            this.imported_search_row.title = "Search Rules";
            this.imported_search_row.notify["text"].connect (this.refresh_imported_rules);
            search_group.add (this.imported_search_row);
            body.append (search_group);
            this.imported_rules_group = new Adw.PreferencesGroup ();
            this.imported_rules_group.title = "Imported Rules";
            body.append (this.imported_rules_group);
            this.imported_load_more_button = new Gtk.Button.with_label ("Load More");
            this.imported_load_more_button.halign = Gtk.Align.CENTER;
            this.imported_load_more_button.clicked.connect (this.append_imported_rule_batch);
            body.append (this.imported_load_more_button);
            var scrolled = this.wrap_scrolled (body);
            this.connect_imported_scroll_loader (scrolled);
            Gtk.Label unused;
            return this.with_subpage_header (scrolled, "Rules", "detail", out unused);
        }

        private Gtk.Widget build_custom_rules_page () {
            var body = new Gtk.Box (Gtk.Orientation.VERTICAL, 12);
            this.prepare_list_body (body);
            var search_group = new Adw.PreferencesGroup ();
            this.custom_search_row = new Adw.EntryRow ();
            this.custom_search_row.title = "Search Rules";
            this.custom_search_row.notify["text"].connect (this.refresh_custom_rules);
            search_group.add (this.custom_search_row);
            body.append (search_group);
            this.custom_rules_group = new Adw.PreferencesGroup ();
            this.custom_rules_group.title = "Custom Rules";
            var actions = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            this.clear_custom_button = new Gtk.Button.with_label ("Clear");
            this.clear_custom_button.add_css_class ("destructive-action");
            this.clear_custom_button.clicked.connect (this.confirm_clear_custom_rules);
            actions.append (this.clear_custom_button);
            var add_button = new Gtk.Button.with_label ("Add Rule");
            add_button.add_css_class ("suggested-action");
            add_button.clicked.connect (this.show_add_dialog);
            actions.append (add_button);
            this.custom_rules_group.header_suffix = actions;
            body.append (this.custom_rules_group);
            this.custom_load_more_button = new Gtk.Button.with_label ("Load More");
            this.custom_load_more_button.halign = Gtk.Align.CENTER;
            this.custom_load_more_button.clicked.connect (this.append_custom_rule_batch);
            body.append (this.custom_load_more_button);
            var scrolled = this.wrap_scrolled (body);
            this.connect_custom_scroll_loader (scrolled);
            Gtk.Label unused;
            return this.with_subpage_header (scrolled, "Custom Rules", "overview", out unused);
        }

        private void prepare_list_body (Gtk.Box body) {
            body.margin_start = 18;
            body.margin_end = 18;
            body.margin_top = 18;
            body.margin_bottom = 18;
        }

        private Gtk.ScrolledWindow wrap_scrolled (Gtk.Widget child) {
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            var clamp = new Adw.Clamp ();
            clamp.maximum_size = 820;
            clamp.tightening_threshold = 620;
            clamp.set_child (child);
            scrolled.set_child (clamp);
            return scrolled;
        }

        private Gtk.Widget with_subpage_header (
            Gtk.Widget content, string title, string back_page, out Gtk.Label title_label
        ) {
            var page = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.margin_start = 12;
            header.margin_end = 12;
            header.margin_top = 8;
            header.margin_bottom = 8;
            var back = new Gtk.Button.from_icon_name ("go-previous-symbolic");
            back.tooltip_text = "Back";
            back.add_css_class ("flat");
            back.clicked.connect (() => { this.page_stack.visible_child_name = back_page; });
            header.append (back);
            title_label = new Gtk.Label (title);
            title_label.add_css_class ("title-4");
            title_label.halign = Gtk.Align.START;
            header.append (title_label);
            page.append (header);
            page.append (new Gtk.Separator (Gtk.Orientation.HORIZONTAL));
            page.append (content);
            return page;
        }

        private Adw.ActionRow create_navigation_row (string title, string subtitle, string icon_name) {
            var row = new Adw.ActionRow ();
            row.title = title;
            row.subtitle = subtitle;
            row.activatable = true;
            row.add_prefix (new Gtk.Image.from_icon_name (icon_name));
            row.add_suffix (new Gtk.Image.from_icon_name ("go-next-symbolic"));
            return row;
        }

        private void refresh_overview () {
            this.refreshing = true;
            this.default_policy_row.selected =
                this.config_manager.get_domain_default_policy () == "proxy" ? 1 : 0;
            foreach (var row in this.configuration_rows) this.configurations_group.remove (row);
            this.configuration_rows.remove_range (0, this.configuration_rows.length);
            var sources = this.config_manager.get_rule_sources ();
            string active_id = this.config_manager.get_active_rule_source_id ();
            foreach (var source in sources) {
                string source_id = source.id;
                string subtitle = "Local Configuration";
                if (source.updated_at > 0) {
                    var updated = new GLib.DateTime.from_unix_local (source.updated_at);
                    subtitle = updated.format ("%Y-%m-%d %H:%M");
                }
                var row = this.create_navigation_row (
                    source.name, subtitle,
                    source_id == active_id ? "object-select-symbolic" : "text-x-generic-symbolic"
                );
                row.activated.connect (() => {
                    this.config_manager.set_active_rule_source (source_id);
                    this.page_stack.visible_child_name = "detail";
                });
                this.configurations_group.add (row);
                this.configuration_rows.add (row);
            }
            if (sources.length == 0) {
                var empty = new Adw.ActionRow ();
                empty.title = "No Configurations";
                this.configurations_group.add (empty);
                this.configuration_rows.add (empty);
            }
            this.custom_summary_row.subtitle = "%u rules".printf (
                this.config_manager.get_domain_rules ().length
            );
            this.refreshing = false;
        }

        private void refresh_default_policy () {
            this.refreshing = true;
            this.default_policy_row.selected =
                this.config_manager.get_domain_default_policy () == "proxy" ? 1 : 0;
            this.refreshing = false;
        }

        private void refresh_detail () {
            this.refreshing = true;
            string name = this.config_manager.get_rule_source_name ();
            this.detail_title.label = name != "" ? name : "Configuration";
            uint direct;
            uint proxy;
            uint reject;
            this.config_manager.get_imported_rule_counts (out direct, out proxy, out reject);
            this.source_row.title = name != "" ? name : "Imported Configuration";
            string source_url = this.config_manager.get_rule_source_url ();
            this.source_row.subtitle = source_url != "" ? source_url : "Local File";
            this.update_source_button.visible = source_url != "";
            this.remove_source_button.visible = this.config_manager.get_imported_rule_count () > 0;
            this.imported_summary_row.subtitle = "%u rules · %u direct · %u proxy · %u reject".printf (
                this.config_manager.get_imported_rule_count (), direct, proxy, reject
            );
            this.refreshing = false;
        }

        private bool rule_matches_query (DomainRule rule, string query) {
            return query == "" || query in rule.pattern || query in rule.action || query in rule.rule_type;
        }

        private void refresh_imported_rules () {
            if (this.imported_search_row == null) return;
            string query = this.imported_search_row.text.strip ().down ();
            this.filtered_imported_rules.remove_range (0, this.filtered_imported_rules.length);
            foreach (var rule in this.config_manager.get_imported_domain_rules ()) {
                if (this.rule_matches_query (rule, query)) this.filtered_imported_rules.add (rule);
            }
            this.clear_rule_rows (this.imported_rules_group, this.imported_rule_rows);
            this.imported_loaded_count = 0;
            this.append_imported_rule_batch ();
        }

        private void append_imported_rule_batch () {
            uint end = uint.min (
                this.imported_loaded_count + RULE_BATCH_SIZE, this.filtered_imported_rules.length
            );
            while (this.imported_loaded_count < end) {
                var row = this.create_rule_row (
                    this.filtered_imported_rules[this.imported_loaded_count], false
                );
                this.imported_rules_group.add (row);
                this.imported_rule_rows.add (row);
                this.imported_loaded_count++;
            }
            this.imported_load_more_button.visible =
                this.imported_loaded_count < this.filtered_imported_rules.length;
        }

        private void refresh_custom_rules () {
            if (this.custom_search_row == null) return;
            string query = this.custom_search_row.text.strip ().down ();
            this.filtered_custom_rules.remove_range (0, this.filtered_custom_rules.length);
            foreach (var rule in this.config_manager.get_domain_rules ()) {
                if (this.rule_matches_query (rule, query)) this.filtered_custom_rules.add (rule);
            }
            this.clear_rule_rows (this.custom_rules_group, this.custom_rule_rows);
            this.custom_loaded_count = 0;
            this.clear_custom_button.visible = this.config_manager.get_domain_rules ().length > 0;
            this.append_custom_rule_batch ();
        }

        private void append_custom_rule_batch () {
            uint end = uint.min (
                this.custom_loaded_count + RULE_BATCH_SIZE, this.filtered_custom_rules.length
            );
            while (this.custom_loaded_count < end) {
                var row = this.create_rule_row (
                    this.filtered_custom_rules[this.custom_loaded_count], true
                );
                this.custom_rules_group.add (row);
                this.custom_rule_rows.add (row);
                this.custom_loaded_count++;
            }
            this.custom_load_more_button.visible =
                this.custom_loaded_count < this.filtered_custom_rules.length;
        }

        private Adw.ActionRow create_rule_row (DomainRule rule, bool custom) {
            var row = new Adw.ActionRow ();
            row.title = GLib.Markup.escape_text (rule.pattern);
            row.subtitle = @"$(rule.rule_type.up ()) · $(rule.action.up ())";
            row.add_prefix (new Gtk.Image.from_icon_name (
                rule.action == "proxy" ? "ssh-rocket-symbolic" :
                (rule.action == "reject" ? "network-offline-symbolic" : "network-wired-symbolic")
            ));
            if (custom) {
                var edit_button = new Gtk.Button.from_icon_name ("document-edit-symbolic");
                edit_button.tooltip_text = "Edit Rule";
                edit_button.valign = Gtk.Align.CENTER;
                edit_button.add_css_class ("flat");
                edit_button.clicked.connect (() => this.show_edit_dialog (rule));
                row.add_suffix (edit_button);
                var delete_button = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                delete_button.tooltip_text = "Delete Rule";
                delete_button.valign = Gtk.Align.CENTER;
                delete_button.add_css_class ("flat");
                string pattern = rule.pattern;
                delete_button.clicked.connect (() => this.config_manager.remove_domain_rule (pattern));
                row.add_suffix (delete_button);
            }
            return row;
        }

        private void clear_rule_rows (
            Adw.PreferencesGroup group, GLib.GenericArray<Gtk.Widget> rows
        ) {
            foreach (var row in rows) group.remove (row);
            rows.remove_range (0, rows.length);
        }

        private void connect_imported_scroll_loader (Gtk.ScrolledWindow scrolled) {
            scrolled.vadjustment.value_changed.connect (() => {
                var adjustment = scrolled.vadjustment;
                if (adjustment.value + adjustment.page_size >= adjustment.upper - 160) {
                    this.append_imported_rule_batch ();
                }
            });
        }

        private void connect_custom_scroll_loader (Gtk.ScrolledWindow scrolled) {
            scrolled.vadjustment.value_changed.connect (() => {
                var adjustment = scrolled.vadjustment;
                if (adjustment.value + adjustment.page_size >= adjustment.upper - 160) {
                    this.append_custom_rule_batch ();
                }
            });
        }

        private void show_add_dialog () { this.show_rule_dialog (null); }
        private void show_edit_dialog (DomainRule rule) { this.show_rule_dialog (rule); }

        private void show_rule_dialog (DomainRule? rule) {
            var dialog = new Adw.AlertDialog (rule == null ? "Add Rule" : "Edit Rule", null);
            var group = new Adw.PreferencesGroup ();
            var pattern_row = new Adw.EntryRow ();
            pattern_row.title = "Domain, IP, or CIDR";
            pattern_row.text = rule != null ? rule.pattern : "";
            group.add (pattern_row);
            var type_row = new Adw.ComboRow ();
            type_row.title = "Rule Type";
            type_row.model = Native.string_list ({
                "DOMAIN-SUFFIX", "DOMAIN", "DOMAIN-KEYWORD", "IP-CIDR"
            });
            if (rule != null) {
                type_row.selected = rule.rule_type == "domain" ? 1 :
                    (rule.rule_type == "domain-keyword" ? 2 :
                    (rule.rule_type == "ip-cidr" ? 3 : 0));
            }
            group.add (type_row);
            var action_row = new Adw.ComboRow ();
            action_row.title = "Action";
            action_row.model = Native.string_list ({ "DIRECT", "PROXY", "REJECT" });
            action_row.selected = rule == null || rule.action == "proxy" ? 1 :
                (rule.action == "reject" ? 2 : 0);
            group.add (action_row);
            dialog.set_extra_child (group);
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("save", "Save");
            dialog.set_response_appearance ("save", Adw.ResponseAppearance.SUGGESTED);
            dialog.response.connect ((response) => {
                string pattern = pattern_row.text.strip ();
                if (response != "save" || pattern == "") return;
                string[] actions = { "direct", "proxy", "reject" };
                string[] types = { "domain-suffix", "domain", "domain-keyword", "ip-cidr" };
                if (rule == null) {
                    this.config_manager.add_domain_rule (
                        pattern, actions[action_row.selected], types[type_row.selected]
                    );
                } else {
                    this.config_manager.update_domain_rule (
                        rule.pattern, pattern, actions[action_row.selected], types[type_row.selected]
                    );
                }
            });
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void set_source_busy (bool busy) {
            this.source_spinner.visible = busy;
            this.source_spinner.spinning = busy;
            this.update_source_button.sensitive = !busy;
            this.remove_source_button.sensitive = !busy;
        }

        private void show_import_dialog () {
            var dialog = new Adw.AlertDialog ("Import Configuration", null);
            var group = new Adw.PreferencesGroup ();
            var url_row = new Adw.EntryRow ();
            url_row.title = "HTTPS URL";
            url_row.text = RuleImporter.DEFAULT_SOURCE_URL;
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
                        imported, url, name != "" ? name : "Imported Configuration"
                    );
                }
                this.page_stack.visible_child_name = "detail";
                this.show_import_result (imported);
            } catch (GLib.Error e) {
                this.show_message ("Import Failed", e.message);
            } finally {
                this.set_source_busy (false);
            }
        }

        private void choose_rule_file () {
            var dialog = new Gtk.FileDialog ();
            dialog.title = "Import Configuration";
            var filter = new Gtk.FileFilter ();
            filter.name = "Rule Config (*.conf, *.txt, *.json, *.bak)";
            foreach (var pattern in new string[] { "*.conf", "*.txt", "*.json", "*.bak" }) {
                filter.add_pattern (pattern);
            }
            var filters = new GLib.ListStore (typeof (Gtk.FileFilter));
            filters.append (filter);
            dialog.filters = filters;
            dialog.open.begin (this.get_root () as Gtk.Window, null, (obj, result) => {
                try {
                    var file = dialog.open.end (result);
                    string? path = file.get_path ();
                    if (path != null) this.import_file.begin (
                        path, file.get_basename () ?? "Imported Configuration"
                    );
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
                this.page_stack.visible_child_name = "detail";
                this.show_import_result (imported);
            } catch (GLib.Error e) {
                this.show_message ("Import Failed", e.message);
            } finally {
                this.set_source_busy (false);
            }
        }

        private void show_import_result (RuleImportResult result) {
            this.show_message (
                "Configuration Imported",
                "%u direct · %u proxy · %u reject".printf (
                    result.direct_count, result.proxy_count, result.reject_count
                )
            );
        }

        private void show_message (string title, string message) {
            var dialog = new Adw.AlertDialog (title, message);
            dialog.add_response ("close", "Close");
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void confirm_remove_source () {
            var dialog = new Adw.AlertDialog ("Remove Configuration?", null);
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("remove", "Remove");
            dialog.set_response_appearance ("remove", Adw.ResponseAppearance.DESTRUCTIVE);
            dialog.response.connect ((response) => {
                if (response == "remove") {
                    this.config_manager.clear_imported_rule_source ();
                    this.page_stack.visible_child_name = "overview";
                }
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
    }
}

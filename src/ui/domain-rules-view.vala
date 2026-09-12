namespace Sshuttle {

    public class DomainRulesView : Adw.PreferencesGroup {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Adw.ComboRow default_policy_row;
        private Adw.EntryRow new_pattern_row;
        private Gtk.DropDown action_dropdown;
        private Adw.EntryRow search_row;
        private Gtk.Box rules_list_box;

        private GLib.GenericArray<Adw.ActionRow> rule_rows;
        private GLib.GenericArray<DomainRule> displayed_rules;

        public DomainRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.rule_rows = new GLib.GenericArray<Adw.ActionRow> ();
            this.displayed_rules = new GLib.GenericArray<DomainRule> ();

            this.title = "Domain Routing and Exceptions (Zero Omega Compatible)";
            this.description = GLib.Markup.escape_text ("Route specific domains via direct exception or proxy. Supports wildcards (*.google.com).");

            // 1. 默认兜底策略
            this.default_policy_row = new Adw.ComboRow ();
            this.default_policy_row.title = "Default Policy for Unmatched Domains";
            this.default_policy_row.subtitle = "Strategy for unlisted domains";
            string[] policies = { "direct", "proxy" };
            this.default_policy_row.model = new Gtk.StringList (policies);

            bool is_def_proxy = (this.config_manager.get_domain_default_policy () == "proxy");
            this.default_policy_row.selected = is_def_proxy ? 1 : 0;
            this.default_policy_row.notify["selected"].connect (() => {
                string pol = (this.default_policy_row.selected == 1) ? "proxy" : "direct";
                this.config_manager.set_domain_default_policy (pol);
            });
            this.add (this.default_policy_row);

            // 2. 导入与操作工具栏行
            var tools_row = new Adw.ActionRow ();
            tools_row.title = "Manage Rules";
            tools_row.subtitle = "Import from Zero Omega / SwitchyOmega backup or text rule lists";

            var import_btn = new Gtk.Button.with_label ("Import Config…");
            import_btn.valign = Gtk.Align.CENTER;
            import_btn.clicked.connect (this.on_import_clicked);
            tools_row.add_suffix (import_btn);

            var clear_btn = new Gtk.Button.with_label ("Clear All");
            clear_btn.valign = Gtk.Align.CENTER;
            clear_btn.add_css_class ("destructive-action");
            clear_btn.clicked.connect (() => {
                this.config_manager.clear_domain_rules ();
            });
            tools_row.add_suffix (clear_btn);

            this.add (tools_row);

            // 3. 新增域名规则行
            this.new_pattern_row = new Adw.EntryRow ();
            this.new_pattern_row.title = "New Domain Pattern (e.g. *.google.com)";

            string[] action_labels = { "direct", "proxy" };
            this.action_dropdown = new Gtk.DropDown.from_strings (action_labels);
            this.action_dropdown.selected = 0;
            this.action_dropdown.valign = Gtk.Align.CENTER;
            this.new_pattern_row.add_suffix (this.action_dropdown);

            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.valign = Gtk.Align.CENTER;
            add_btn.add_css_class ("suggested-action");
            add_btn.clicked.connect (this.on_add_rule_clicked);
            this.new_pattern_row.add_suffix (add_btn);
            this.new_pattern_row.entry_activated.connect (this.on_add_rule_clicked);

            this.add (this.new_pattern_row);

            // 4. 搜索框
            this.search_row = new Adw.EntryRow ();
            this.search_row.title = "Search Domain Rules";
            this.search_row.notify["text"].connect (this.filter_rules);
            this.add (this.search_row);

            // 5. 规则列表展示
            this.rules_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            this.rules_list_box.margin_top = 8;
            this.add (this.rules_list_box);

            this.config_manager.domain_rules_changed.connect (this.refresh_rules_list);
            this.refresh_rules_list ();
        }

        private void on_add_rule_clicked () {
            string pattern = this.new_pattern_row.text.strip ();
            if (pattern == "") {
                return;
            }

            string action = (this.action_dropdown.selected == 0) ? "direct" : "proxy";
            this.config_manager.add_domain_rule (pattern, action);
            this.new_pattern_row.text = "";
        }

        private void on_import_clicked () {
            var dialog = new Gtk.FileDialog ();
            dialog.title = "Import Zero Omega / SwitchyOmega Config";

            var filter_all = new Gtk.FileFilter ();
            filter_all.name = "All Supported (*.bak, *.json, *.txt)";
            filter_all.add_pattern ("*.bak");
            filter_all.add_pattern ("*.json");
            filter_all.add_pattern ("*.txt");

            var filters = new GLib.ListStore (typeof (Gtk.FileFilter));
            filters.append (filter_all);
            dialog.filters = filters;

            var root_win = this.get_root () as Gtk.Window;
            dialog.open.begin (root_win, null, (obj, res) => {
                try {
                    var file = dialog.open.end (res);
                    if (file != null) {
                        string path = file.get_path ();
                        var imported = OmegaImporter.import_from_file (path);
                        if (imported != null && imported.rules.length > 0) {
                            var arr = new DomainRule[imported.rules.length];
                            for (uint i = 0; i < imported.rules.length; i++) {
                                arr[i] = imported.rules[i];
                            }
                            this.config_manager.set_domain_rules (arr, imported.default_policy);
                            this.default_policy_row.selected = (imported.default_policy == "proxy") ? 1 : 0;
                        }
                    }
                } catch (GLib.Error e) {
                    // 取消或打开错误
                }
            });
        }

        private void filter_rules () {
            string query = this.search_row.text.strip ().down ();
            for (uint i = 0; i < this.displayed_rules.length; i++) {
                var r = this.displayed_rules[i];
                var row = this.rule_rows[i];
                if (query == "") {
                    row.visible = true;
                } else {
                    row.visible = (query in r.pattern || query in r.action);
                }
            }
        }

        private void show_edit_dialog (DomainRule rule) {
            var root_win = this.get_root () as Gtk.Window;
            var dialog = new Adw.MessageDialog (root_win, "Edit Domain Rule", null);

            var group = new Adw.PreferencesGroup ();

            var pat_entry = new Adw.EntryRow ();
            pat_entry.title = "Pattern";
            pat_entry.text = rule.pattern;
            group.add (pat_entry);

            var act_row = new Adw.ComboRow ();
            act_row.title = "Action";
            string[] acts = { "direct", "proxy" };
            act_row.model = new Gtk.StringList (acts);
            act_row.selected = (rule.action == "proxy") ? 1 : 0;
            group.add (act_row);

            dialog.set_extra_child (group);

            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("save", "Save");
            dialog.set_response_appearance ("save", Adw.ResponseAppearance.SUGGESTED);

            dialog.response.connect ((resp) => {
                if (resp == "save") {
                    string np = pat_entry.text.strip ().down ();
                    string na = (act_row.selected == 1) ? "proxy" : "direct";
                    if (np != "") {
                        this.config_manager.update_domain_rule (rule.pattern, np, na);
                    }
                }
            });

            dialog.present ();
        }

        private void refresh_rules_list () {
            // 清空列表与引用
            Gtk.Widget? child = this.rules_list_box.get_first_child ();
            while (child != null) {
                Gtk.Widget next = child.get_next_sibling ();
                this.rules_list_box.remove (child);
                child = next;
            }

            this.rule_rows.remove_range (0, this.rule_rows.length);
            this.displayed_rules.remove_range (0, this.displayed_rules.length);

            var rules = this.config_manager.get_domain_rules ();
            foreach (var rule in rules) {
                var row = new Adw.ActionRow ();
                row.title = GLib.Markup.escape_text (rule.pattern);
                row.subtitle = rule.action; // direct 或 proxy

                // 标记图标
                var icon = new Gtk.Image ();
                icon.valign = Gtk.Align.CENTER;
                if (rule.action == "proxy") {
                    icon.icon_name = "network-vpn-symbolic";
                    icon.add_css_class ("success");
                } else {
                    icon.icon_name = "network-wired-symbolic";
                    icon.add_css_class ("dim-label");
                }
                row.add_prefix (icon);

                // 编辑按钮
                var edit_btn = new Gtk.Button.from_icon_name ("document-edit-symbolic");
                edit_btn.valign = Gtk.Align.CENTER;
                edit_btn.add_css_class ("flat");
                var current_rule = rule;
                edit_btn.clicked.connect (() => {
                    this.show_edit_dialog (current_rule);
                });
                row.add_suffix (edit_btn);

                // 行激活时也可以直接打开编辑
                row.activatable = true;
                row.activated.connect (() => {
                    this.show_edit_dialog (current_rule);
                });

                // 删除按钮
                var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                del_btn.valign = Gtk.Align.CENTER;
                del_btn.add_css_class ("flat");
                string pat = rule.pattern;
                del_btn.clicked.connect (() => {
                    this.config_manager.remove_domain_rule (pat);
                });
                row.add_suffix (del_btn);

                this.rules_list_box.append (row);
                this.rule_rows.add (row);
                this.displayed_rules.add (rule);
            }

            this.filter_rules ();
        }
    }
}

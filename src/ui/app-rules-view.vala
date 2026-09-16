namespace SshRocket {

    public class AppRuleRow : Adw.ActionRow {
        public AppInfo app { get; private set; }
        public Gtk.Switch toggle_switch { get; private set; }
        public uint64 total_traffic { get; set; default = 0; }

        public AppRuleRow (AppInfo app, Gtk.Switch sw) {
            this.app = app;
            this.toggle_switch = sw;
        }
    }

    public class AppRulesView : Adw.PreferencesGroup {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Gtk.SearchEntry search_entry;
        private Gtk.DropDown sort_drop;
        private Gtk.ListBox apps_list_box;
        private GLib.GenericArray<AppRuleRow> app_rows;
        private GLib.GenericArray<AppInfo> apps;

        public AppRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.app_rows = new GLib.GenericArray<AppRuleRow> ();
            this.apps = new GLib.GenericArray<AppInfo> ();

            // 搜索与排序放在同一行
            var toolbar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            toolbar.margin_top = 4;
            toolbar.margin_bottom = 4;

            this.search_entry = new Gtk.SearchEntry ();
            this.search_entry.placeholder_text = "Search";
            this.search_entry.hexpand = true;
            this.search_entry.search_changed.connect (() => {
                this.apps_list_box.invalidate_filter ();
            });
            toolbar.append (this.search_entry);

            this.sort_drop = new Gtk.DropDown (Native.string_list ({ "Name", "Usage" }), null);
            this.sort_drop.notify["selected"].connect (() => {
                this.apps_list_box.invalidate_sort ();
            });

            var sort_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            var sort_label = new Gtk.Label ("Sort");
            sort_label.add_css_class ("dim-label");
            sort_box.append (sort_label);
            sort_box.append (this.sort_drop);
            toolbar.append (sort_box);

            this.add (toolbar);

            this.apps_list_box = new Gtk.ListBox ();
            this.apps_list_box.add_css_class ("boxed-list");
            this.apps_list_box.selection_mode = Gtk.SelectionMode.NONE;
            this.apps_list_box.margin_top = 8;
            this.apps_list_box.set_filter_func (this.filter_app_row);
            this.apps_list_box.set_sort_func (this.sort_apps_func);
            this.add (this.apps_list_box);

            this.config_manager.traffic_stats_changed.connect (this.update_traffic_display);
            this.config_manager.app_rules_changed.connect (this.refresh_selection);

            this.load_apps_async.begin ();
        }

        private async void load_apps_async () {
            // 扫描已安装应用
            this.apps = AppScanner.scan_apps ();

            for (uint i = 0; i < this.apps.length; i++) {
                var app = this.apps[i];

                // 勾选切换开关：默认未勾选；勾选后走代理
                var sw = new Gtk.Switch ();
                sw.valign = Gtk.Align.CENTER;
                bool is_checked = this.config_manager.is_app_proxied (app.id) || this.config_manager.is_app_proxied (app.exec_name);
                sw.active = is_checked;

                var row = new AppRuleRow (app, sw);
                row.title = GLib.Markup.escape_text (app.name);
                row.subtitle = GLib.Markup.escape_text (app.exec_name);

                // 设置应用图标
                if (app.icon_name != "") {
                    Gtk.Image? img = null;
                    if (app.icon_name.has_prefix ("/") && GLib.FileUtils.test (app.icon_name, GLib.FileTest.EXISTS)) {
                        var icon_file = GLib.File.new_for_path (app.icon_name);
                        var gicon = new GLib.FileIcon (icon_file);
                        img = new Gtk.Image.from_gicon (gicon);
                    } else {
                        img = new Gtk.Image.from_icon_name (app.icon_name);
                    }

                    if (img != null) {
                        img.pixel_size = 28;
                        img.valign = Gtk.Align.CENTER;
                        row.add_prefix (img);
                    }
                }

                string app_id = app.id;
                sw.notify["active"].connect (() => {
                    this.config_manager.set_app_proxied (app_id, sw.active);
                    this.apps_list_box.invalidate_sort ();
                });

                row.add_suffix (sw);
                row.activatable_widget = sw;

                this.apps_list_box.append (row);
                this.app_rows.add (row);
            }

            this.update_traffic_display ();
        }

        public void update_traffic_display () {
            for (uint i = 0; i < this.app_rows.length; i++) {
                var row = this.app_rows[i];
                var app = row.app;
                uint64 up, down;
                this.config_manager.get_app_traffic (app.id, out up, out down);
                if (up == 0 && down == 0) {
                    this.config_manager.get_app_traffic (app.exec_name, out up, out down);
                }
                row.total_traffic = up + down;
                if (up > 0 || down > 0) {
                    row.subtitle = @"$(app.exec_name)  •  ↑ $(TunnelManager.format_bytes (up))   ↓ $(TunnelManager.format_bytes (down))";
                } else {
                    row.subtitle = @"$(app.exec_name)  •  ↑ 0 B   ↓ 0 B";
                }
            }

            if (this.sort_drop.selected == 1) {
                this.apps_list_box.invalidate_sort ();
            }
        }

        /**
         * 根据当前配置刷新应用开关状态。
         */
        private void refresh_selection () {
            for (uint i = 0; i < this.app_rows.length; i++) {
                var row = this.app_rows[i];
                row.toggle_switch.active = this.config_manager.is_app_proxied (row.app.id) ||
                                           this.config_manager.is_app_proxied (row.app.exec_name);
            }
            this.apps_list_box.invalidate_sort ();
        }

        private int sort_apps_func (Gtk.ListBoxRow row_a, Gtk.ListBoxRow row_b) {
            var a = row_a as AppRuleRow;
            var b = row_b as AppRuleRow;
            if (a == null || b == null) {
                return 0;
            }

            // 1. 勾选后的应用默认排在最前面
            bool a_checked = a.toggle_switch.active;
            bool b_checked = b.toggle_switch.active;
            if (a_checked != b_checked) {
                return a_checked ? -1 : 1;
            }

            // 2. 勾选状态相同时，根据排序选项排序 (0: Name, 1: Usage)
            if (this.sort_drop.selected == 1) {
                if (a.total_traffic != b.total_traffic) {
                    return (a.total_traffic > b.total_traffic) ? -1 : 1;
                }
            }

            return a.app.name.collate (b.app.name);
        }

        private bool filter_app_row (Gtk.ListBoxRow list_row) {
            var row = list_row as AppRuleRow;
            if (row == null) {
                return true;
            }

            string query = this.search_entry.text.strip ().down ();
            if (query == "") {
                return true;
            }

            return (row.app.name.down ().contains (query) ||
                    row.app.exec_name.down ().contains (query) ||
                    row.app.id.down ().contains (query));
        }
    }
}

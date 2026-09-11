namespace Sshuttle {

    public class AppRulesView : Adw.PreferencesGroup {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Adw.EntryRow search_row;
        private Gtk.Box apps_list_box;
        private GLib.GenericArray<Adw.ActionRow> app_rows;
        private GLib.GenericArray<AppInfo> apps;

        public AppRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.app_rows = new GLib.GenericArray<Adw.ActionRow> ();
            this.apps = new GLib.GenericArray<AppInfo> ();

            this.title = "App Proxy Rules";
            this.description = GLib.Markup.escape_text ("Only checked applications will route through proxy. Unchecked applications connect directly.");

            // 搜索框
            this.search_row = new Adw.EntryRow ();
            this.search_row.title = "Search Applications (e.g. WeChat, Firefox)";
            this.search_row.notify["text"].connect (this.filter_apps);
            this.add (this.search_row);

            this.apps_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            this.apps_list_box.margin_top = 8;
            this.add (this.apps_list_box);

            this.load_apps_async.begin ();
        }

        private async void load_apps_async () {
            // 扫描已安装应用
            this.apps = AppScanner.scan_apps ();

            for (uint i = 0; i < this.apps.length; i++) {
                var app = this.apps[i];
                var row = new Adw.ActionRow ();
                row.title = GLib.Markup.escape_text (app.name);
                row.subtitle = GLib.Markup.escape_text (app.exec_name);

                // 设置应用图标
                if (app.icon_name != "") {
                    Gtk.Image? img = null;
                    if (app.icon_name.has_prefix ("/") && GLib.FileUtils.test (app.icon_name, GLib.FileTest.EXISTS)) {
                        try {
                            var icon_file = GLib.File.new_for_path (app.icon_name);
                            var gicon = new GLib.FileIcon (icon_file);
                            img = new Gtk.Image.from_gicon (gicon);
                        } catch (GLib.Error e) {
                        }
                    } else {
                        img = new Gtk.Image.from_icon_name (app.icon_name);
                    }

                    if (img != null) {
                        img.pixel_size = 28;
                        img.valign = Gtk.Align.CENTER;
                        row.add_prefix (img);
                    }
                }

                // 勾选切换开关：默认未勾选（不走代理）；勾选后走代理
                var sw = new Gtk.Switch ();
                sw.valign = Gtk.Align.CENTER;
                bool is_checked = this.config_manager.is_app_proxied (app.id) || this.config_manager.is_app_proxied (app.exec_name);
                sw.active = is_checked;

                string app_id = app.id;
                sw.notify["active"].connect (() => {
                    this.config_manager.set_app_proxied (app_id, sw.active);
                });

                row.add_suffix (sw);
                row.activatable_widget = sw;

                this.apps_list_box.append (row);
                this.app_rows.add (row);
            }
        }

        private void filter_apps () {
            string query = this.search_row.text.strip ().down ();
            for (uint i = 0; i < this.apps.length; i++) {
                var app = this.apps[i];
                var row = this.app_rows[i];

                if (query == "") {
                    row.visible = true;
                } else {
                    bool match = (app.name.down ().contains (query) ||
                                  app.exec_name.down ().contains (query) ||
                                  app.id.down ().contains (query));
                    row.visible = match;
                }
            }
        }
    }
}

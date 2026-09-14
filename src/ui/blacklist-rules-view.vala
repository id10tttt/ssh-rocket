namespace Sshuttle {

    public class BlacklistRulesView : Adw.PreferencesGroup {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Adw.EntryRow search_row;
        private Gtk.Box apps_list_box;
        private GLib.GenericArray<Adw.ActionRow> app_rows;
        private GLib.GenericArray<AppInfo> apps;

        private Gtk.Box procs_list_box;
        private Adw.EntryRow new_proc_row;

        public BlacklistRulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.app_rows = new GLib.GenericArray<Adw.ActionRow> ();
            this.apps = new GLib.GenericArray<AppInfo> ();

            this.title = "Network Blacklist";
            this.description = GLib.Markup.escape_text ("Prohibit selected applications and processes from accessing the network completely.");

            // ===== 进程黑名单输入 =====
            var proc_group_header = new Adw.PreferencesGroup ();
            proc_group_header.title = "Blocked Processes";
            proc_group_header.description = "Block standalone processes or background daemons by executable name.";
            this.add (proc_group_header);

            this.procs_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            proc_group_header.add (this.procs_list_box);

            this.new_proc_row = new Adw.EntryRow ();
            this.new_proc_row.title = "Add Process Name (e.g. curl, wget)";
            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.add_css_class ("flat");
            add_btn.valign = Gtk.Align.CENTER;
            add_btn.clicked.connect (this.on_add_process);
            this.new_proc_row.add_suffix (add_btn);
            this.new_proc_row.entry_activated.connect (this.on_add_process);
            proc_group_header.add (this.new_proc_row);

            this.refresh_process_list ();

            // ===== 应用程序黑名单 =====
            var apps_group_header = new Adw.PreferencesGroup ();
            apps_group_header.title = "Blocked Applications";
            apps_group_header.description = "Switch on to completely block an application from internet access.";
            this.add (apps_group_header);

            // 搜索框
            this.search_row = new Adw.EntryRow ();
            this.search_row.title = "Search Applications (e.g. WeChat, Firefox)";
            this.search_row.notify["text"].connect (this.filter_apps);
            apps_group_header.add (this.search_row);

            this.apps_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            this.apps_list_box.margin_top = 8;
            apps_group_header.add (this.apps_list_box);

            this.config_manager.blacklist_changed.connect (() => {
                this.refresh_process_list ();
                this.refresh_app_selection ();
            });
            this.load_apps_async.begin ();
        }

        private void refresh_process_list () {
            // 清理旧行
            Gtk.Widget? child = this.procs_list_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.procs_list_box.remove (child);
                child = next;
            }

            string[] procs = this.config_manager.get_blocked_processes ();
            for (uint i = 0; i < procs.length; i++) {
                string proc_name = procs[i];
                var row = new Adw.ActionRow ();
                row.title = GLib.Markup.escape_text (proc_name);
                row.subtitle = "Blocked from network";

                var icon = new Gtk.Image.from_icon_name ("network-offline-symbolic");
                icon.valign = Gtk.Align.CENTER;
                row.add_prefix (icon);

                var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                del_btn.add_css_class ("flat");
                del_btn.add_css_class ("destructive-action");
                del_btn.valign = Gtk.Align.CENTER;

                string target = proc_name;
                del_btn.clicked.connect (() => {
                    this.config_manager.remove_blocked_process (target);
                    this.refresh_process_list ();
                });
                row.add_suffix (del_btn);

                this.procs_list_box.append (row);
            }
        }

        private void on_add_process () {
            string proc = this.new_proc_row.text.strip ();
            if (proc == "") {
                return;
            }
            this.config_manager.add_blocked_process (proc);
            this.new_proc_row.text = "";
            this.refresh_process_list ();
        }

        private async void load_apps_async () {
            this.apps = AppScanner.scan_apps ();

            for (uint i = 0; i < this.apps.length; i++) {
                var app = this.apps[i];
                var row = new Adw.ActionRow ();
                row.title = GLib.Markup.escape_text (app.name);
                row.subtitle = GLib.Markup.escape_text (app.exec_name);

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

                var sw = new Gtk.Switch ();
                sw.valign = Gtk.Align.CENTER;
                bool is_blocked = this.config_manager.is_app_blocked (app.id) || this.config_manager.is_app_blocked (app.exec_name);
                sw.active = is_blocked;

                string app_id = app.id;
                sw.notify["active"].connect (() => {
                    this.config_manager.set_app_blocked (app_id, sw.active);
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

        /**
         * 根据当前配置刷新应用黑名单开关。
         */
        private void refresh_app_selection () {
            for (uint i = 0; i < this.apps.length && i < this.app_rows.length; i++) {
                var sw = this.app_rows[i].activatable_widget as Gtk.Switch;
                if (sw != null) {
                    sw.active = this.config_manager.is_app_blocked (this.apps[i].id) ||
                                this.config_manager.is_app_blocked (this.apps[i].exec_name);
                }
            }
        }
    }
}

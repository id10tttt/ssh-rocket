namespace Sshuttle {

    public class RulesView : Gtk.Box {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Gtk.Box exclude_list_box;
        private Adw.EntryRow new_cidr_row;
        private GLib.GenericArray<string> global_excludes;

        public RulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.global_excludes = new GLib.GenericArray<string> ();

            this.load_excludes ();

            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            this.append (scrolled);

            var clamp = new Adw.Clamp ();
            clamp.maximum_size = 620;
            clamp.tightening_threshold = 400;
            scrolled.set_child (clamp);

            var page = new Adw.PreferencesPage ();
            clamp.set_child (page);

            // 按软件代理规则 (Per-App Proxy)
            var app_rules_group = new AppRulesView (this.config_manager, this.tunnel_manager);
            page.add (app_rules_group);

            // 常用预设网段分组
            var presets_group = new Adw.PreferencesGroup ();
            presets_group.title = "Common Private Networks";
            page.add (presets_group);

            this.add_preset_row (presets_group, "192.168.0.0/16", "Local Class C");
            this.add_preset_row (presets_group, "10.0.0.0/8", "Local Class A");
            this.add_preset_row (presets_group, "172.16.0.0/12", "Local Class B");
            this.add_preset_row (presets_group, "127.0.0.0/8", "Loopback");

            // 自定义直连排除网络 (Exclude Networks)
            var exclude_group = new Adw.PreferencesGroup ();
            exclude_group.title = "Active Exclude Networks (Bypass Proxy)";
            page.add (exclude_group);

            this.exclude_list_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 6);
            exclude_group.add (this.exclude_list_box);

            this.new_cidr_row = new Adw.EntryRow ();
            this.new_cidr_row.title = "Add CIDR (e.g. 192.168.1.0/24)";
            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.add_css_class ("flat");
            add_btn.valign = Gtk.Align.CENTER;
            add_btn.clicked.connect (this.on_add_cidr);
            this.new_cidr_row.add_suffix (add_btn);
            this.new_cidr_row.entry_activated.connect (this.on_add_cidr);
            exclude_group.add (this.new_cidr_row);

            this.refresh_excludes ();
        }

        private void add_preset_row (Adw.PreferencesGroup group, string cidr, string desc) {
            var row = new Adw.ActionRow ();
            row.title = cidr;
            row.subtitle = desc;

            var add_btn = new Gtk.Button.with_label ("Add");
            add_btn.add_css_class ("flat");
            add_btn.valign = Gtk.Align.CENTER;
            add_btn.clicked.connect (() => {
                this.add_exclude_item (cidr);
            });
            row.add_suffix (add_btn);
            group.add (row);
        }

        private void load_excludes () {
            this.global_excludes.remove_range (0, this.global_excludes.length);
            var active_p = this.config_manager.get_active_profile ();
            if (active_p != null) {
                foreach (var exc in active_p.exclude) {
                    this.global_excludes.add (exc);
                }
            } else {
                this.global_excludes.add ("192.168.0.0/16");
                this.global_excludes.add ("10.0.0.0/8");
            }
        }

        private void refresh_excludes () {
            var child = this.exclude_list_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.exclude_list_box.remove (child);
                child = next;
            }

            for (uint i = 0; i < this.global_excludes.length; i++) {
                string item = this.global_excludes[i];
                var row = new Adw.ActionRow ();
                row.title = item;

                var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                del_btn.add_css_class ("flat");
                del_btn.valign = Gtk.Align.CENTER;
                del_btn.clicked.connect (() => {
                    for (uint j = 0; j < this.global_excludes.length; j++) {
                        if (this.global_excludes[j] == item) {
                            this.global_excludes.remove_index (j);
                            break;
                        }
                    }
                    this.sync_to_active_profile ();
                    this.refresh_excludes ();
                });
                row.add_suffix (del_btn);
                this.exclude_list_box.append (row);
            }
        }

        private void add_exclude_item (string cidr) {
            string trimmed = cidr.strip ();
            if (trimmed != "") {
                for (uint i = 0; i < this.global_excludes.length; i++) {
                    if (this.global_excludes[i] == trimmed) {
                        return;
                    }
                }
                this.global_excludes.add (trimmed);
                this.sync_to_active_profile ();
                this.refresh_excludes ();
            }
        }

        private void on_add_cidr () {
            string text = this.new_cidr_row.text.strip ();
            if (text != "") {
                this.add_exclude_item (text);
                this.new_cidr_row.text = "";
            }
        }

        private void sync_to_active_profile () {
            var active_p = this.config_manager.get_active_profile ();
            if (active_p != null) {
                var arr = new string[this.global_excludes.length];
                for (uint i = 0; i < this.global_excludes.length; i++) {
                    arr[i] = this.global_excludes[i];
                }
                active_p.exclude = arr;
                this.config_manager.save_profile (active_p);
            }
        }
    }
}

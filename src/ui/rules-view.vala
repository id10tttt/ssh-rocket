namespace Sshuttle {

    public class RulesView : Gtk.Box {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;

        private Adw.ViewStack sub_stack;
        private Gtk.Box exclude_list_box;
        private Adw.EntryRow new_cidr_row;
        private GLib.GenericArray<string> global_excludes;

        public RulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;
            this.global_excludes = new GLib.GenericArray<string> ();

            this.load_excludes ();

            // Tab 切换栏 (ViewSwitcher)
            var switcher_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            switcher_box.halign = Gtk.Align.CENTER;
            switcher_box.margin_top = 8;
            switcher_box.margin_bottom = 8;

            this.sub_stack = new Adw.ViewStack ();
            this.sub_stack.vexpand = true;

            var switcher = new Adw.ViewSwitcher ();
            switcher.stack = this.sub_stack;
            switcher.policy = Adw.ViewSwitcherPolicy.NARROW;
            switcher_box.append (switcher);
            this.append (switcher_box);
            this.append (this.sub_stack);

            // Tab 1: 按软件规则 (Applications)
            var app_scrolled = new Gtk.ScrolledWindow ();
            app_scrolled.vexpand = true;

            var app_clamp = new Adw.Clamp ();
            app_clamp.maximum_size = 780;
            app_clamp.tightening_threshold = 560;
            app_scrolled.set_child (app_clamp);

            var app_page = new Adw.PreferencesPage ();
            app_clamp.set_child (app_page);

            var app_rules_group = new AppRulesView (this.config_manager, this.tunnel_manager);
            app_page.add (app_rules_group);

            var app_vs_page = this.sub_stack.add_named (app_scrolled, "apps");
            app_vs_page.title = "Applications";
            app_vs_page.icon_name = "application-x-executable-symbolic";

            // Tab 2: 域名与IP规则 (Domain & IP)
            var routing_scrolled = new Gtk.ScrolledWindow ();
            routing_scrolled.vexpand = true;

            var routing_clamp = new Adw.Clamp ();
            routing_clamp.maximum_size = 780;
            routing_clamp.tightening_threshold = 560;
            routing_scrolled.set_child (routing_clamp);

            var routing_page = new Adw.PreferencesPage ();
            routing_clamp.set_child (routing_page);

            // 域名通配符规则 (Domain Wildcards / Zero Omega)
            var domain_rules_group = new DomainRulesView (this.config_manager, this.tunnel_manager);
            routing_page.add (domain_rules_group);

            // 常用预设网段分组
            var presets_group = new Adw.PreferencesGroup ();
            presets_group.title = "Common Private Networks";
            routing_page.add (presets_group);

            this.add_preset_row (presets_group, "192.168.0.0/16", "Local Class C");
            this.add_preset_row (presets_group, "10.0.0.0/8", "Local Class A");
            this.add_preset_row (presets_group, "172.16.0.0/12", "Local Class B");
            this.add_preset_row (presets_group, "127.0.0.0/8", "Loopback");

            // 自定义直连排除网络 (Exclude Networks)
            var exclude_group = new Adw.PreferencesGroup ();
            exclude_group.title = "Active Exclude Networks (Bypass Proxy)";
            routing_page.add (exclude_group);

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

            var routing_vs_page = this.sub_stack.add_named (routing_scrolled, "routing");
            routing_vs_page.title = "Domain & IP";
            routing_vs_page.icon_name = "network-server-symbolic";

            // Tab 3: 黑名单规则 (Blacklist)
            var blacklist_scrolled = new Gtk.ScrolledWindow ();
            blacklist_scrolled.vexpand = true;

            var blacklist_clamp = new Adw.Clamp ();
            blacklist_clamp.maximum_size = 780;
            blacklist_clamp.tightening_threshold = 560;
            blacklist_scrolled.set_child (blacklist_clamp);

            var blacklist_page = new Adw.PreferencesPage ();
            blacklist_clamp.set_child (blacklist_page);

            var blacklist_rules_group = new BlacklistRulesView (this.config_manager, this.tunnel_manager);
            blacklist_page.add (blacklist_rules_group);

            var blacklist_vs_page = this.sub_stack.add_named (blacklist_scrolled, "blacklist");
            blacklist_vs_page.title = "Blacklist";
            blacklist_vs_page.icon_name = "network-offline-symbolic";

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
                if (!this.is_valid_network (text)) {
                    var root_win = this.get_root () as Gtk.Window;
                    var dialog = new Adw.AlertDialog ("Invalid Network", "Enter a valid IPv4 or IPv6 address with an optional CIDR prefix.");
                    dialog.add_response ("close", "Close");
                    dialog.present (root_win);
                    return;
                }
                this.add_exclude_item (text);
                this.new_cidr_row.text = "";
            }
        }

        /**
         * 校验直连网络地址及 CIDR 前缀。
         */
        private bool is_valid_network (string value) {
            string[] parts = value.strip ().split ("/", 2);
            if (parts.length == 0 || parts[0] == "") {
                return false;
            }
            var address = new GLib.InetAddress.from_string (parts[0]);
            if (address == null) {
                return false;
            }
            if (parts.length == 1) {
                return true;
            }
            int prefix;
            int max_prefix = address.get_family () == GLib.SocketFamily.IPV6 ? 128 : 32;
            return int.try_parse (parts[1], out prefix) && prefix >= 0 && prefix <= max_prefix;
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
                this.tunnel_manager.refresh_routing_configuration ();
            }
        }
    }
}

namespace Sshuttle {

    public class SettingsView : Gtk.Box {
        private ConfigManager config_manager;
        private Adw.EntryRow routes_row;
        private Adw.SwitchRow dns_row;
        private Adw.SwitchRow ipv6_row;
        private Adw.ComboRow verbosity_row;
        private Adw.SwitchRow auto_connect_row;
        private Gtk.Box exclude_rows_box;
        private Adw.EntryRow new_exclude_row;
        private GLib.GenericArray<string> excludes;
        private bool refreshing = false;

        private static string[] VERBOSITIES = { "normal", "verbose", "very_verbose" };

        public SettingsView (ConfigManager config_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.config_manager = config_manager;
            this.excludes = new GLib.GenericArray<string> ();
            this.vexpand = true;

            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            this.append (scrolled);

            var clamp = new Adw.Clamp ();
            clamp.maximum_size = 760;
            clamp.tightening_threshold = 560;
            scrolled.set_child (clamp);

            var page = new Adw.PreferencesPage ();
            clamp.set_child (page);

            var routing_group = new Adw.PreferencesGroup ();
            routing_group.title = "Routing";
            routing_group.description = "Changes apply on the next connection.";
            page.add (routing_group);

            this.routes_row = new Adw.EntryRow ();
            this.routes_row.title = "Proxy Routes";
            var save_routes_button = new Gtk.Button.with_label ("Apply");
            save_routes_button.valign = Gtk.Align.CENTER;
            save_routes_button.add_css_class ("suggested-action");
            save_routes_button.clicked.connect (this.save_routes);
            this.routes_row.add_suffix (save_routes_button);
            this.routes_row.entry_activated.connect (this.save_routes);
            routing_group.add (this.routes_row);

            this.dns_row = new Adw.SwitchRow ();
            this.dns_row.title = "Remote DNS";
            this.dns_row.notify["active"].connect (this.save_switches);
            routing_group.add (this.dns_row);

            this.ipv6_row = new Adw.SwitchRow ();
            this.ipv6_row.title = "IPv6";
            this.ipv6_row.notify["active"].connect (this.save_switches);
            routing_group.add (this.ipv6_row);

            var exclude_group = new Adw.PreferencesGroup ();
            exclude_group.title = "Direct Networks";
            page.add (exclude_group);

            this.exclude_rows_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            exclude_group.add (this.exclude_rows_box);

            this.new_exclude_row = new Adw.EntryRow ();
            this.new_exclude_row.title = "IP Address or CIDR";
            var add_exclude_button = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_exclude_button.valign = Gtk.Align.CENTER;
            add_exclude_button.add_css_class ("flat");
            add_exclude_button.clicked.connect (this.add_exclude);
            this.new_exclude_row.add_suffix (add_exclude_button);
            this.new_exclude_row.entry_activated.connect (this.add_exclude);
            exclude_group.add (this.new_exclude_row);

            var behavior_group = new Adw.PreferencesGroup ();
            behavior_group.title = "Connection";
            page.add (behavior_group);

            this.auto_connect_row = new Adw.SwitchRow ();
            this.auto_connect_row.title = "Connect on Launch";
            this.auto_connect_row.notify["active"].connect (this.save_behavior);
            behavior_group.add (this.auto_connect_row);

            this.verbosity_row = new Adw.ComboRow ();
            this.verbosity_row.title = "Log Level";
            this.verbosity_row.model = Native.string_list ({ "Normal", "Verbose", "Debug" });
            this.verbosity_row.notify["selected"].connect (this.save_behavior);
            behavior_group.add (this.verbosity_row);

            var data_group = new Adw.PreferencesGroup ();
            data_group.title = "Data";
            page.add (data_group);

            var reset_row = new Adw.ActionRow ();
            reset_row.title = "Reset Rules and Settings";
            var reset_button = new Gtk.Button.with_label ("Reset…");
            reset_button.valign = Gtk.Align.CENTER;
            reset_button.add_css_class ("destructive-action");
            reset_button.clicked.connect (this.confirm_reset);
            reset_row.add_suffix (reset_button);
            data_group.add (reset_row);

            this.config_manager.network_settings_changed.connect (this.refresh);
            this.refresh ();
        }

        private void refresh () {
            this.refreshing = true;
            var settings = this.config_manager.get_network_settings ();
            this.routes_row.text = string.joinv (", ", settings.routes);
            this.dns_row.active = settings.dns;
            this.ipv6_row.active = settings.ipv6;
            this.auto_connect_row.active = settings.auto_connect;
            this.verbosity_row.selected = settings.verbosity == "very_verbose" ? 2 :
                (settings.verbosity == "verbose" ? 1 : 0);
            this.excludes.remove_range (0, this.excludes.length);
            foreach (var network in settings.exclude) {
                this.excludes.add (network);
            }
            this.refresh_excludes ();
            this.refreshing = false;
        }

        private void save_routes () {
            var routes = this.parse_networks (this.routes_row.text);
            if (routes == null) {
                this.show_invalid_network ();
                return;
            }
            if (routes.length == 0) {
                routes = new string[] { "0.0.0.0/0" };
                this.routes_row.text = routes[0];
            }
            var settings = this.config_manager.get_network_settings ();
            settings.routes = routes;
            this.config_manager.set_network_settings (settings);
        }

        private void save_switches () {
            if (this.refreshing) return;
            var settings = this.config_manager.get_network_settings ();
            settings.dns = this.dns_row.active;
            settings.ipv6 = this.ipv6_row.active;
            this.config_manager.set_network_settings (settings);
        }

        private void save_behavior () {
            if (this.refreshing) return;
            var settings = this.config_manager.get_network_settings ();
            settings.auto_connect = this.auto_connect_row.active;
            uint index = this.verbosity_row.selected;
            settings.verbosity = index < VERBOSITIES.length ? VERBOSITIES[index] : "normal";
            this.config_manager.set_network_settings (settings);
        }

        private void refresh_excludes () {
            Gtk.Widget? child = this.exclude_rows_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.exclude_rows_box.remove (child);
                child = next;
            }
            for (uint i = 0; i < this.excludes.length; i++) {
                string network = this.excludes[i];
                var row = new Adw.ActionRow ();
                row.title = network;
                var remove_button = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                remove_button.valign = Gtk.Align.CENTER;
                remove_button.add_css_class ("flat");
                remove_button.clicked.connect (() => this.remove_exclude (network));
                row.add_suffix (remove_button);
                this.exclude_rows_box.append (row);
            }
        }

        private void add_exclude () {
            string network = this.new_exclude_row.text.strip ();
            if (!this.is_valid_network (network)) {
                this.show_invalid_network ();
                return;
            }
            for (uint i = 0; i < this.excludes.length; i++) {
                if (this.excludes[i] == network) return;
            }
            this.excludes.add (network);
            this.new_exclude_row.text = "";
            this.save_excludes ();
        }

        private void remove_exclude (string network) {
            for (uint i = 0; i < this.excludes.length; i++) {
                if (this.excludes[i] == network) {
                    this.excludes.remove_index (i);
                    break;
                }
            }
            this.save_excludes ();
        }

        private void save_excludes () {
            var values = new string[this.excludes.length];
            for (uint i = 0; i < this.excludes.length; i++) values[i] = this.excludes[i];
            var settings = this.config_manager.get_network_settings ();
            settings.exclude = values;
            this.config_manager.set_network_settings (settings);
        }

        private string[]? parse_networks (string value) {
            var result = new GLib.GenericArray<string> ();
            foreach (var item in value.split (",")) {
                string network = item.strip ();
                if (network == "") continue;
                if (!this.is_valid_network (network)) return null;
                result.add (network);
            }
            var values = new string[result.length];
            for (uint i = 0; i < result.length; i++) values[i] = result[i];
            return values;
        }

        private bool is_valid_network (string value) {
            string[] parts = value.strip ().split ("/", 2);
            if (parts.length == 0 || parts[0] == "") return false;
            var address = new GLib.InetAddress.from_string (parts[0]);
            if (address == null) return false;
            if (parts.length == 1) return true;
            int prefix;
            int max_prefix = address.get_family () == GLib.SocketFamily.IPV6 ? 128 : 32;
            return int.try_parse (parts[1], out prefix) && prefix >= 0 && prefix <= max_prefix;
        }

        private void show_invalid_network () {
            var dialog = new Adw.AlertDialog ("Invalid Network", "Enter a valid IP address or CIDR.");
            dialog.add_response ("close", "Close");
            dialog.present (this.get_root () as Gtk.Window);
        }

        private void confirm_reset () {
            var dialog = new Adw.AlertDialog (
                "Reset Rules and Settings?",
                "Connection profiles will be kept."
            );
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("reset", "Reset");
            dialog.set_response_appearance ("reset", Adw.ResponseAppearance.DESTRUCTIVE);
            dialog.default_response = "cancel";
            dialog.close_response = "cancel";
            dialog.response.connect ((response) => {
                if (response == "reset") this.config_manager.reset_rules_and_settings ();
            });
            dialog.present (this.get_root () as Gtk.Window);
        }
    }
}

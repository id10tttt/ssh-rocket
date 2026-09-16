namespace SshRocket {

    /** 当前连接与全部累计共用的流量统计页面。 */
    public class TrafficView : Gtk.Box {
        private const uint PAGE_SIZE = 20;
        private TunnelManager tunnel_manager;
        private ConfigManager config_manager;
        private Gtk.Stack page_stack;
        private Adw.ComboRow range_row;
        private Adw.ActionRow started_row;
        private Adw.ActionRow duration_row;
        private Gtk.Label upload_value;
        private Gtk.Label download_value;
        private Gtk.Label total_value;
        private Adw.ActionRow config_row;
        private Adw.ActionRow proxy_row;
        private Adw.ActionRow direct_row;
        private Adw.ActionRow reject_row;
        private Adw.ActionRow apps_row;
        private Adw.ActionRow domains_row;
        private Adw.EntryRow app_search_row;
        private Adw.ComboRow app_sort_row;
        private Adw.PreferencesGroup app_list_group;
        private Gtk.Button app_load_more;
        private Adw.EntryRow domain_search_row;
        private Adw.ComboRow domain_sort_row;
        private Adw.PreferencesGroup domain_list_group;
        private Gtk.Button domain_load_more;
        private GLib.GenericArray<Gtk.Widget> app_rows;
        private GLib.GenericArray<Gtk.Widget> domain_rows;
        private GLib.GenericArray<AppTrafficStats> filtered_apps;
        private GLib.GenericArray<DomainTrafficStats> filtered_domains;
        private GLib.HashTable<string, AppInfo> app_index;
        private uint app_loaded = 0;
        private uint domain_loaded = 0;

        public TrafficView (TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.tunnel_manager = tunnel_manager;
            this.config_manager = tunnel_manager.config_manager;
            this.app_rows = new GLib.GenericArray<Gtk.Widget> ();
            this.domain_rows = new GLib.GenericArray<Gtk.Widget> ();
            this.filtered_apps = new GLib.GenericArray<AppTrafficStats> ();
            this.filtered_domains = new GLib.GenericArray<DomainTrafficStats> ();
            this.app_index = new GLib.HashTable<string, AppInfo> (GLib.str_hash, GLib.str_equal);
            foreach (var app in AppScanner.scan_apps ()) {
                this.app_index.insert (app.id, app);
                this.app_index.insert (app.exec_name, app);
            }

            this.page_stack = new Gtk.Stack ();
            this.page_stack.transition_type = Gtk.StackTransitionType.SLIDE_LEFT_RIGHT;
            this.page_stack.vexpand = true;
            this.append (this.page_stack);
            this.page_stack.add_named (this.build_overview (), "overview");
            this.page_stack.add_named (this.build_app_page (), "apps");
            this.page_stack.add_named (this.build_domain_page (), "domains");
            this.page_stack.visible_child_name = "overview";

            this.range_row.notify["selected"].connect (() => {
                this.refresh_visible ();
            });
            this.tunnel_manager.traffic_stats_updated.connect (() => {
                if (this.get_mapped ()) this.refresh_overview ();
            });
            this.notify["mapped"].connect (() => {
                if (this.get_mapped ()) this.refresh_visible ();
            });
            GLib.Timeout.add_seconds (1, () => {
                if (this.get_mapped ()) this.refresh_overview ();
                return GLib.Source.CONTINUE;
            });
            this.refresh_overview ();
        }

        private Gtk.Widget build_overview () {
            var page = new Adw.PreferencesPage ();
            var range_group = new Adw.PreferencesGroup ();
            range_group.title = "Statistics Range";
            this.range_row = new Adw.ComboRow ();
            this.range_row.title = "Range";
            this.range_row.model = Native.string_list ({ "Current Session", "All Time" });
            range_group.add (this.range_row);
            page.add (range_group);

            var session_group = new Adw.PreferencesGroup ();
            session_group.title = "Connection";
            this.started_row = this.create_value_row ("Started");
            this.duration_row = this.create_value_row ("Connected Time");
            session_group.add (this.started_row);
            session_group.add (this.duration_row);
            page.add (session_group);

            var traffic_group = new Adw.PreferencesGroup ();
            traffic_group.title = "Traffic";

            var traffic_cards = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 12);
            traffic_cards.homogeneous = true;

            this.upload_value = new Gtk.Label ("—");
            traffic_cards.append (this.create_stat_card ("↑", "Uploaded", this.upload_value));
            this.download_value = new Gtk.Label ("—");
            traffic_cards.append (this.create_stat_card ("↓", "Downloaded", this.download_value));
            this.total_value = new Gtk.Label ("—");
            traffic_cards.append (this.create_stat_card ("Σ", "Total", this.total_value));

            traffic_group.add (traffic_cards);
            page.add (traffic_group);

            var config_group = new Adw.PreferencesGroup ();
            config_group.title = "Configuration";
            this.config_row = this.create_value_row ("Active Configuration");
            this.proxy_row = this.create_value_row ("PROXY");
            this.direct_row = this.create_value_row ("DIRECT");
            this.reject_row = this.create_value_row ("REJECT");
            config_group.add (this.config_row);
            config_group.add (this.proxy_row);
            config_group.add (this.direct_row);
            config_group.add (this.reject_row);
            page.add (config_group);

            var details_group = new Adw.PreferencesGroup ();
            details_group.title = "Details";
            this.apps_row = this.create_navigation_row ("Applications", "0 applications");
            this.apps_row.activated.connect (() => {
                this.refresh_apps ();
                this.page_stack.visible_child_name = "apps";
            });
            this.domains_row = this.create_navigation_row ("Domains", "0 domains");
            this.domains_row.activated.connect (() => {
                this.refresh_domains ();
                this.page_stack.visible_child_name = "domains";
            });
            details_group.add (this.apps_row);
            details_group.add (this.domains_row);
            page.add (details_group);
            return this.wrap_scrolled (page);
        }

        private Gtk.Widget build_app_page () {
            var body = this.create_list_body ();
            var search_group = new Adw.PreferencesGroup ();
            search_group.title = "Search";
            this.app_search_row = new Adw.EntryRow ();
            this.app_search_row.title = "Application Name";
            this.app_search_row.notify["text"].connect (this.refresh_apps);
            search_group.add (this.app_search_row);
            body.append (search_group);
            var sort_group = new Adw.PreferencesGroup ();
            sort_group.title = "Sort";
            this.app_sort_row = new Adw.ComboRow ();
            this.app_sort_row.title = "Sort By";
            this.app_sort_row.model = Native.string_list ({ "Traffic", "Name" });
            this.app_sort_row.notify["selected"].connect (this.refresh_apps);
            sort_group.add (this.app_sort_row);
            body.append (sort_group);
            this.app_list_group = new Adw.PreferencesGroup ();
            this.app_list_group.title = "Application Traffic";
            body.append (this.app_list_group);
            this.app_load_more = new Gtk.Button.with_label ("Load More");
            this.app_load_more.halign = Gtk.Align.CENTER;
            this.app_load_more.clicked.connect (this.append_app_batch);
            body.append (this.app_load_more);
            var scrolled = this.wrap_scrolled (body);
            scrolled.vadjustment.value_changed.connect (() => {
                var a = scrolled.vadjustment;
                if (a.value + a.page_size >= a.upper - 160) this.append_app_batch ();
            });
            return this.with_header (scrolled, "Application Traffic");
        }

        private Gtk.Widget build_domain_page () {
            var body = this.create_list_body ();
            var search_group = new Adw.PreferencesGroup ();
            search_group.title = "Search";
            this.domain_search_row = new Adw.EntryRow ();
            this.domain_search_row.title = "Domain";
            this.domain_search_row.notify["text"].connect (this.refresh_domains);
            search_group.add (this.domain_search_row);
            body.append (search_group);
            var sort_group = new Adw.PreferencesGroup ();
            sort_group.title = "Sort";
            this.domain_sort_row = new Adw.ComboRow ();
            this.domain_sort_row.title = "Sort By";
            this.domain_sort_row.model = Native.string_list ({ "Traffic", "Requests", "Name" });
            this.domain_sort_row.notify["selected"].connect (this.refresh_domains);
            sort_group.add (this.domain_sort_row);
            body.append (sort_group);
            this.domain_list_group = new Adw.PreferencesGroup ();
            this.domain_list_group.title = "Domain Traffic";
            body.append (this.domain_list_group);
            this.domain_load_more = new Gtk.Button.with_label ("Load More");
            this.domain_load_more.halign = Gtk.Align.CENTER;
            this.domain_load_more.clicked.connect (this.append_domain_batch);
            body.append (this.domain_load_more);
            var scrolled = this.wrap_scrolled (body);
            scrolled.vadjustment.value_changed.connect (() => {
                var a = scrolled.vadjustment;
                if (a.value + a.page_size >= a.upper - 160) this.append_domain_batch ();
            });
            return this.with_header (scrolled, "Domain Traffic");
        }

        private Gtk.Box create_list_body () {
            var body = new Gtk.Box (Gtk.Orientation.VERTICAL, 12);
            body.margin_start = 18;
            body.margin_end = 18;
            body.margin_top = 18;
            body.margin_bottom = 18;
            return body;
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

        private Gtk.Widget with_header (Gtk.Widget content, string title) {
            var page = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.margin_start = 12;
            header.margin_end = 12;
            header.margin_top = 8;
            header.margin_bottom = 8;
            var back = new Gtk.Button.from_icon_name ("go-previous-symbolic");
            back.tooltip_text = "Back";
            back.add_css_class ("flat");
            back.clicked.connect (() => { this.page_stack.visible_child_name = "overview"; });
            header.append (back);
            var label = new Gtk.Label (title);
            label.add_css_class ("title-4");
            header.append (label);
            page.append (header);
            page.append (new Gtk.Separator (Gtk.Orientation.HORIZONTAL));
            page.append (content);
            return page;
        }

        private Adw.ActionRow create_value_row (string title) {
            var row = new Adw.ActionRow ();
            row.title = title;
            row.subtitle = "—";
            return row;
        }

        /** 创建流量统计卡片，包含符号、标题和数值标签。 */
        private Gtk.Widget create_stat_card (string symbol, string title, Gtk.Label value_label) {
            var card = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            card.hexpand = true;
            card.add_css_class ("card");
            card.margin_top = 8;
            card.margin_bottom = 8;

            var inner = new Gtk.Box (Gtk.Orientation.VERTICAL, 2);
            inner.margin_start = 12;
            inner.margin_end = 12;
            inner.margin_top = 12;
            inner.margin_bottom = 12;
            inner.halign = Gtk.Align.CENTER;

            var symbol_label = new Gtk.Label (symbol);
            symbol_label.add_css_class ("title-2");
            inner.append (symbol_label);

            var title_label = new Gtk.Label (title);
            title_label.add_css_class ("dim-label");
            title_label.add_css_class ("caption");
            inner.append (title_label);

            value_label.add_css_class ("title-4");
            inner.append (value_label);

            card.append (inner);
            return card;
        }

        private Adw.ActionRow create_navigation_row (string title, string subtitle) {
            var row = new Adw.ActionRow ();
            row.title = title;
            row.subtitle = subtitle;
            row.activatable = true;
            row.add_suffix (new Gtk.Image.from_icon_name ("go-next-symbolic"));
            return row;
        }

        private bool all_time () { return this.range_row.selected == 1; }

        private void refresh_visible () {
            this.refresh_overview ();
            if (this.page_stack.visible_child_name == "apps") this.refresh_apps ();
            else if (this.page_stack.visible_child_name == "domains") this.refresh_domains ();
        }

        private void refresh_overview () {
            int64 started = this.all_time ()
                ? this.config_manager.get_traffic_first_recorded_at ()
                : this.tunnel_manager.get_session_started_at ();
            this.started_row.subtitle = this.format_timestamp (started);
            uint64 duration = this.all_time ()
                ? this.tunnel_manager.get_all_time_connected_seconds ()
                : this.tunnel_manager.get_session_connected_seconds ();
            this.duration_row.subtitle = this.format_duration (duration);
            uint64 up;
            uint64 down;
            if (this.all_time ()) this.config_manager.get_total_traffic (out up, out down);
            else this.tunnel_manager.get_session_total_traffic (out up, out down);
            this.upload_value.label = TunnelManager.format_bytes (up);
            this.download_value.label = TunnelManager.format_bytes (down);
            this.total_value.label = TunnelManager.format_bytes (up + down);
            string config_name = this.config_manager.get_rule_source_name ();
            this.config_row.subtitle = config_name != "" ? config_name : "No Configuration";

            var domains = this.get_domain_entries ();
            uint64 proxy_up = 0, proxy_down = 0, proxy_hits = 0;
            uint64 direct_up = 0, direct_down = 0, direct_hits = 0;
            uint64 reject_hits = 0;
            foreach (var item in domains) {
                if (item.action == "direct") {
                    direct_up += item.bytes_uploaded;
                    direct_down += item.bytes_downloaded;
                    direct_hits += item.hits;
                } else if (item.action == "reject") {
                    reject_hits += item.hits;
                } else {
                    proxy_up += item.bytes_uploaded;
                    proxy_down += item.bytes_downloaded;
                    proxy_hits += item.hits;
                }
            }
            this.proxy_row.subtitle = this.format_action (proxy_hits, proxy_up, proxy_down);
            this.direct_row.subtitle = this.format_action (direct_hits, direct_up, direct_down);
            this.reject_row.subtitle = "%lu requests".printf ((ulong) reject_hits);
            this.apps_row.subtitle = "%u applications".printf (this.get_app_entries ().length);
            this.domains_row.subtitle = "%u domains".printf (domains.length);
        }

        private string format_action (uint64 hits, uint64 up, uint64 down) {
            return "%lu requests · ↑ %s · ↓ %s".printf (
                (ulong) hits, TunnelManager.format_bytes (up), TunnelManager.format_bytes (down)
            );
        }

        private AppTrafficStats[] get_app_entries () {
            return this.all_time () ? this.config_manager.get_app_traffic_entries ()
                                   : this.tunnel_manager.get_session_app_traffic_entries ();
        }

        private DomainTrafficStats[] get_domain_entries () {
            return this.all_time () ? this.config_manager.get_domain_traffic_entries ()
                                   : this.tunnel_manager.get_session_domain_traffic_entries ();
        }

        private void refresh_apps () {
            if (this.app_search_row == null) return;
            string query = this.app_search_row.text.strip ().down ();
            this.filtered_apps.remove_range (0, this.filtered_apps.length);
            foreach (var item in this.get_app_entries ()) {
                string name = this.get_app_name (item.app_id);
                if (query == "" || query in name.down () || query in item.app_id.down ()) {
                    this.filtered_apps.add (item);
                }
            }
            if (this.app_sort_row.selected == 0) {
                this.filtered_apps.sort ((a, b) => {
                    uint64 a_total = a.bytes_uploaded + a.bytes_downloaded;
                    uint64 b_total = b.bytes_uploaded + b.bytes_downloaded;
                    return a_total == b_total ? a.app_id.collate (b.app_id) : (a_total > b_total ? -1 : 1);
                });
            } else {
                this.sort_filtered_apps_by_name ();
            }
            this.clear_rows (this.app_list_group, this.app_rows);
            this.app_loaded = 0;
            this.append_app_batch ();
        }

        private void append_app_batch () {
            uint end = uint.min (this.app_loaded + PAGE_SIZE, this.filtered_apps.length);
            while (this.app_loaded < end) {
                var item = this.filtered_apps[this.app_loaded++];
                var row = new Adw.ActionRow ();
                row.title = this.get_app_name (item.app_id);
                row.subtitle = "↑ %s · ↓ %s · Total %s".printf (
                    TunnelManager.format_bytes (item.bytes_uploaded),
                    TunnelManager.format_bytes (item.bytes_downloaded),
                    TunnelManager.format_bytes (item.bytes_uploaded + item.bytes_downloaded)
                );
                row.add_prefix (new Gtk.Image.from_icon_name ("application-x-executable-symbolic"));
                this.app_list_group.add (row);
                this.app_rows.add (row);
            }
            this.app_load_more.visible = this.app_loaded < this.filtered_apps.length;
        }

        private string get_app_name (string app_id) {
            var app = this.app_index.lookup (app_id);
            return app != null ? app.name : app_id;
        }

        private void sort_filtered_apps_by_name () {
            for (uint i = 0; i < this.filtered_apps.length; i++) {
                for (uint j = i + 1; j < this.filtered_apps.length; j++) {
                    if (this.get_app_name (this.filtered_apps[i].app_id).collate (
                            this.get_app_name (this.filtered_apps[j].app_id)
                        ) > 0) {
                        var item = this.filtered_apps[i];
                        this.filtered_apps[i] = this.filtered_apps[j];
                        this.filtered_apps[j] = item;
                    }
                }
            }
        }

        private void refresh_domains () {
            if (this.domain_search_row == null) return;
            string query = this.domain_search_row.text.strip ().down ();
            this.filtered_domains.remove_range (0, this.filtered_domains.length);
            foreach (var item in this.get_domain_entries ()) {
                if (query == "" || query in item.domain) this.filtered_domains.add (item);
            }
            if (this.domain_sort_row.selected == 2) {
                this.filtered_domains.sort ((a, b) => { return a.domain.collate (b.domain); });
            } else if (this.domain_sort_row.selected == 1) {
                this.filtered_domains.sort ((a, b) => {
                    return a.hits == b.hits ? a.domain.collate (b.domain) : (a.hits > b.hits ? -1 : 1);
                });
            } else {
                this.filtered_domains.sort ((a, b) => {
                    uint64 a_total = a.bytes_uploaded + a.bytes_downloaded;
                    uint64 b_total = b.bytes_uploaded + b.bytes_downloaded;
                    return a_total == b_total ? a.domain.collate (b.domain) : (a_total > b_total ? -1 : 1);
                });
            }
            this.clear_rows (this.domain_list_group, this.domain_rows);
            this.domain_loaded = 0;
            this.append_domain_batch ();
        }

        private void append_domain_batch () {
            uint end = uint.min (this.domain_loaded + PAGE_SIZE, this.filtered_domains.length);
            while (this.domain_loaded < end) {
                var item = this.filtered_domains[this.domain_loaded++];
                var row = new Adw.ActionRow ();
                row.title = item.domain;
                row.subtitle = "%s · %lu requests · ↑ %s · ↓ %s".printf (
                    item.action.up (), (ulong) item.hits,
                    TunnelManager.format_bytes (item.bytes_uploaded),
                    TunnelManager.format_bytes (item.bytes_downloaded)
                );
                row.add_prefix (new Gtk.Image.from_icon_name ("network-server-symbolic"));
                this.domain_list_group.add (row);
                this.domain_rows.add (row);
            }
            this.domain_load_more.visible = this.domain_loaded < this.filtered_domains.length;
        }

        private void clear_rows (Adw.PreferencesGroup group, GLib.GenericArray<Gtk.Widget> rows) {
            foreach (var row in rows) group.remove (row);
            rows.remove_range (0, rows.length);
        }

        private string format_timestamp (int64 timestamp) {
            if (timestamp <= 0) return "—";
            return new GLib.DateTime.from_unix_local (timestamp).format ("%Y-%m-%d %H:%M:%S");
        }

        private string format_duration (uint64 seconds) {
            uint64 hours = seconds / 3600;
            uint64 minutes = (seconds % 3600) / 60;
            uint64 remaining = seconds % 60;
            return "%02lu:%02lu:%02lu".printf (
                (ulong) hours, (ulong) minutes, (ulong) remaining
            );
        }
    }
}

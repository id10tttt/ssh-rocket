namespace Sshuttle {

    public class MainWindow : Adw.ApplicationWindow {
        private TunnelManager tunnel_manager;
        private ConfigManager config_manager;

        private Gtk.Stack view_stack;
        private Gtk.FlowBox flow_box;
        private Adw.StatusPage empty_page;
        private Gtk.Stack content_stack;
        private GLib.GenericArray<ConnectionCard> cards;

        private Gtk.Label status_label;
        private Gtk.Label speed_label;

        public MainWindow (Adw.Application app, TunnelManager tunnel_manager) {
            Object (application: app);
            this.tunnel_manager = tunnel_manager;
            this.config_manager = tunnel_manager.config_manager;
            this.cards = new GLib.GenericArray<ConnectionCard> ();

            int w = this.config_manager.get_window_width ();
            int h = this.config_manager.get_window_height ();
            if (w < 820) {
                w = 880;
            }
            if (h < 540) {
                h = 600;
            }
            this.set_default_size (w, h);
            this.title = "SSH Rocket";

            this.setup_actions ();
            this.build_ui ();

            this.tunnel_manager.state_changed.connect (() => {
                this.update_cards_state ();
                this.update_status_display ();
            });

            this.tunnel_manager.profile_changed.connect (() => {
                this.update_cards_state ();
                this.update_status_display ();
            });

            this.tunnel_manager.speed_updated.connect ((up_speed, down_speed) => {
                uint64 total_up, total_down;
                this.config_manager.get_total_traffic (out total_up, out total_down);
                if (total_up > 0 || total_down > 0) {
                    this.speed_label.label = @"↑ $(up_speed) ($(TunnelManager.format_bytes (total_up)))   ↓ $(down_speed) ($(TunnelManager.format_bytes (total_down)))";
                } else {
                    this.speed_label.label = @"↑ $(up_speed)   ↓ $(down_speed)";
                }
            });

            this.config_manager.traffic_stats_changed.connect (() => {
                uint64 total_up, total_down;
                this.config_manager.get_total_traffic (out total_up, out total_down);
                if (total_up > 0 || total_down > 0) {
                    this.speed_label.label = @"↑ $(this.tunnel_manager.current_up_speed) ($(TunnelManager.format_bytes (total_up)))   ↓ $(this.tunnel_manager.current_down_speed) ($(TunnelManager.format_bytes (total_down)))";
                } else {
                    this.speed_label.label = @"↑ $(this.tunnel_manager.current_up_speed)   ↓ $(this.tunnel_manager.current_down_speed)";
                }
            });

            // 点击关闭按钮自动缩放到托盘，不销毁进程
            this.close_request.connect (() => {
                int cur_w, cur_h;
                this.get_default_size (out cur_w, out cur_h);
                this.config_manager.set_window_size (cur_w, cur_h);
                this.set_visible (false);
                return true;
            });
        }

        public void show_and_present () {
            this.set_visible (true);
            this.present ();
        }

        private void setup_actions () {
            var new_profile_action = new GLib.SimpleAction ("new-profile", null);
            new_profile_action.activate.connect (() => {
                this.on_add_profile ();
            });
            this.add_action (new_profile_action);

            var reset_settings_action = new GLib.SimpleAction ("reset-settings", null);
            reset_settings_action.activate.connect (this.on_reset_settings);
            this.add_action (reset_settings_action);
        }

        private void build_ui () {
            var root_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            this.set_content (root_box);

            this.view_stack = new Gtk.Stack ();
            this.view_stack.vexpand = true;
            this.view_stack.hexpand = true;

            var sidebar_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            sidebar_box.width_request = 200;
            sidebar_box.add_css_class ("sidebar");

            var app_title = new Gtk.Label ("SSH Rocket");
            app_title.add_css_class ("title-2");
            app_title.halign = Gtk.Align.START;
            app_title.margin_start = 18;
            app_title.margin_end = 18;
            app_title.margin_top = 18;
            app_title.margin_bottom = 12;
            sidebar_box.append (app_title);

            var navigation_list = new Gtk.ListBox ();
            navigation_list.add_css_class ("navigation-sidebar");
            navigation_list.selection_mode = Gtk.SelectionMode.SINGLE;
            navigation_list.activate_on_single_click = true;
            navigation_list.vexpand = true;

            var connect_nav_row = this.create_navigation_row ("ssh-rocket-symbolic", "Connect");
            var rules_nav_row = this.create_navigation_row ("preferences-system-network-symbolic", "Rules");
            var log_nav_row = this.create_navigation_row ("utilities-terminal-symbolic", "Log");
            navigation_list.append (connect_nav_row);
            navigation_list.append (rules_nav_row);
            navigation_list.append (log_nav_row);
            navigation_list.row_selected.connect ((row) => {
                if (row == connect_nav_row) {
                    this.view_stack.visible_child_name = "connect";
                } else if (row == rules_nav_row) {
                    this.view_stack.visible_child_name = "rules";
                } else if (row == log_nav_row) {
                    this.view_stack.visible_child_name = "log";
                }
            });
            sidebar_box.append (navigation_list);
            root_box.append (sidebar_box);
            root_box.append (new Gtk.Separator (Gtk.Orientation.VERTICAL));

            var toolbar_view = new Adw.ToolbarView ();
            toolbar_view.hexpand = true;
            root_box.append (toolbar_view);

            // 主页面使用左侧导航，HeaderBar 只保留当前页面操作。
            var header_bar = new Adw.HeaderBar ();
            header_bar.set_title_widget (new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0));

            // 右侧操作按钮
            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.tooltip_text = "Add Connection";
            add_btn.add_css_class ("flat");
            add_btn.clicked.connect (this.on_add_profile);
            header_bar.pack_end (add_btn);

            var settings_btn = new Gtk.MenuButton ();
            settings_btn.icon_name = "open-menu-symbolic";
            settings_btn.tooltip_text = "Menu";
            settings_btn.add_css_class ("flat");

            var menu = new GLib.Menu ();
            menu.append ("Reset Rules and Settings…", "win.reset-settings");
            menu.append ("About SSH Rocket", "app.about");
            menu.append ("Quit", "app.quit");
            settings_btn.menu_model = menu;
            header_bar.pack_end (settings_btn);

            toolbar_view.add_top_bar (header_bar);

            // ViewStack 内容承载区
            this.view_stack.vexpand = true;
            toolbar_view.set_content (this.view_stack);

            // Page 1: Connect
            var connect_page = this.build_connect_page ();
            var connect_vs_page = this.view_stack.add_titled (connect_page, "connect", "Connect");
            connect_vs_page.icon_name = "ssh-rocket-symbolic";

            // Page 2: Rules
            var rules_view = new RulesView (this.config_manager, this.tunnel_manager);
            var rules_vs_page = this.view_stack.add_titled (rules_view, "rules", "Rules");
            rules_vs_page.icon_name = "preferences-system-network-symbolic";

            // Page 3: Log
            var log_view = new LogView (this.tunnel_manager);
            var log_vs_page = this.view_stack.add_titled (log_view, "log", "Log");
            log_vs_page.icon_name = "utilities-terminal-symbolic";

            this.view_stack.notify["visible-child-name"].connect (() => {
                add_btn.visible = (this.view_stack.visible_child_name == "connect");
            });

            navigation_list.select_row (connect_nav_row);

            // 底部状态栏 (Bottom Status Bar)
            var bottom_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 12);
            bottom_bar.margin_start = 16;
            bottom_bar.margin_end = 16;
            bottom_bar.margin_top = 8;
            bottom_bar.margin_bottom = 8;

            this.status_label = new Gtk.Label ("Disconnected");
            this.status_label.add_css_class ("dim-label");
            this.status_label.halign = Gtk.Align.START;
            this.status_label.hexpand = true;
            bottom_bar.append (this.status_label);

            this.speed_label = new Gtk.Label ("↑ 0.0 kb/s   ↓ 0.0 kb/s");
            this.speed_label.add_css_class ("dim-label");
            this.speed_label.halign = Gtk.Align.END;
            bottom_bar.append (this.speed_label);

            toolbar_view.add_bottom_bar (bottom_bar);

            this.refresh_connections ();
            this.update_status_display ();
        }

        /**
         * 创建带图标和名称的左侧导航行。
         */
        private Gtk.ListBoxRow create_navigation_row (string icon_name, string title) {
            var row = new Gtk.ListBoxRow ();
            row.height_request = 48;

            var content = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 12);
            content.margin_start = 12;
            content.margin_end = 12;
            content.margin_top = 8;
            content.margin_bottom = 8;

            var icon = new Gtk.Image.from_icon_name (icon_name);
            icon.pixel_size = 20;
            content.append (icon);

            var label = new Gtk.Label (title);
            label.halign = Gtk.Align.START;
            label.hexpand = true;
            content.append (label);

            row.set_child (content);
            return row;
        }

        /**
         * 确认后重置规则和统计设置，连接 Profile 保持不变。
         */
        private void on_reset_settings () {
            var dialog = new Adw.AlertDialog (
                "Reset Rules and Settings?",
                "Application rules, domain rules, blacklist entries, and traffic statistics will be cleared. Connection profiles will be kept."
            );
            dialog.add_response ("cancel", "Cancel");
            dialog.add_response ("reset", "Reset");
            dialog.set_response_appearance ("reset", Adw.ResponseAppearance.DESTRUCTIVE);
            dialog.default_response = "cancel";
            dialog.close_response = "cancel";
            dialog.response.connect ((response) => {
                if (response == "reset") {
                    this.config_manager.reset_rules_and_settings ();
                }
            });
            dialog.present (this);
        }

        private void update_status_display () {
            if (this.status_label == null) {
                return;
            }

            if (this.tunnel_manager.state == TunnelState.CONNECTED) {
                var p = this.tunnel_manager.active_profile;
                string name = (p != null) ? p.name : "Connected";
                this.status_label.label = @"Connected: $(name)";
                this.status_label.remove_css_class ("dim-label");
                this.status_label.add_css_class ("success");
            } else if (this.tunnel_manager.state == TunnelState.CONNECTING) {
                this.status_label.label = "Connecting...";
                this.status_label.remove_css_class ("success");
                this.status_label.add_css_class ("dim-label");
            } else if (this.tunnel_manager.state == TunnelState.ERROR) {
                this.status_label.label = "Connection Error (Auto-reconnecting in 1s...)";
                this.status_label.remove_css_class ("success");
                this.status_label.add_css_class ("dim-label");
            } else {
                this.status_label.label = "Disconnected";
                this.status_label.remove_css_class ("success");
                this.status_label.add_css_class ("dim-label");
                if (this.speed_label != null) {
                    this.speed_label.label = "↑ 0.0 kb/s   ↓ 0.0 kb/s";
                }
            }
        }

        private Gtk.Widget build_connect_page () {
            this.content_stack = new Gtk.Stack ();

            // 空状态占位页
            this.empty_page = new Adw.StatusPage ();
            this.empty_page.icon_name = "ssh-rocket-disconnected-symbolic";
            this.empty_page.title = "No Connections";
            this.empty_page.description = "Add a server to get started.";
            this.empty_page.vexpand = true;

            var empty_add_btn = new Gtk.Button.with_label ("Add Connection");
            empty_add_btn.add_css_class ("pill");
            empty_add_btn.add_css_class ("suggested-action");
            empty_add_btn.halign = Gtk.Align.CENTER;
            empty_add_btn.clicked.connect (this.on_add_profile);
            this.empty_page.set_child (empty_add_btn);

            this.content_stack.add_named (this.empty_page, "empty");

            // 多卡片 FlowBox 容器
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;

            this.flow_box = new Gtk.FlowBox ();
            this.flow_box.valign = Gtk.Align.START;
            this.flow_box.max_children_per_line = 10;
            this.flow_box.min_children_per_line = 1;
            this.flow_box.selection_mode = Gtk.SelectionMode.NONE;
            this.flow_box.margin_start = 24;
            this.flow_box.margin_end = 24;
            this.flow_box.margin_top = 24;
            this.flow_box.margin_bottom = 24;
            this.flow_box.column_spacing = 20;
            this.flow_box.row_spacing = 20;
            this.flow_box.homogeneous = true;

            scrolled.set_child (this.flow_box);
            this.content_stack.add_named (scrolled, "cards");

            return this.content_stack;
        }

        public void refresh_connections () {
            var child = this.flow_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.flow_box.remove (child);
                child = next;
            }
            this.cards.remove_range (0, this.cards.length);

            var profiles = this.config_manager.get_profiles ();
            if (profiles.length == 0) {
                this.content_stack.set_visible_child_name ("empty");
                return;
            }

            this.content_stack.set_visible_child_name ("cards");

            foreach (var p in profiles) {
                var card = new ConnectionCard (p, this.tunnel_manager);

                card.connect_requested.connect ((target_p) => {
                    this.tunnel_manager.set_active_profile (target_p.id);
                    this.tunnel_manager.connect_active ();
                });

                card.disconnect_requested.connect (() => {
                    this.tunnel_manager.disconnect_tunnel ();
                });

                card.edit_requested.connect ((editing_p) => {
                    this.on_edit_profile (editing_p);
                });

                card.delete_requested.connect ((deleting_p) => {
                    this.config_manager.delete_profile (deleting_p.id);
                    this.refresh_connections ();
                });

                this.flow_box.append (card);
                this.cards.add (card);
            }
        }

        private void update_cards_state () {
            for (uint i = 0; i < this.cards.length; i++) {
                this.cards[i].update_state ();
            }
        }

        private void on_add_profile () {
            var editor = new ProfileEditorWindow ();
            editor.profile_saved.connect ((new_p) => {
                this.config_manager.save_profile (new_p);
                this.refresh_connections ();
            });
            editor.present (this);
        }

        private void on_edit_profile (Profile profile) {
            var editor = new ProfileEditorWindow (profile);
            editor.profile_saved.connect ((saved_p) => {
                this.config_manager.save_profile (saved_p);
                this.refresh_connections ();
            });
            editor.profile_deleted.connect ((deleted_p) => {
                this.config_manager.delete_profile (deleted_p.id);
                this.refresh_connections ();
            });
            editor.present (this);
        }
    }
}

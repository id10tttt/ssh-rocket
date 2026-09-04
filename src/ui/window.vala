namespace Sshuttle {

    public class MainWindow : Adw.ApplicationWindow {
        private TunnelManager tunnel_manager;
        private ConfigManager config_manager;

        private Adw.ViewStack view_stack;
        private Gtk.FlowBox flow_box;
        private Adw.StatusPage empty_page;
        private Gtk.Stack content_stack;
        private GLib.GenericArray<ConnectionCard> cards;

        private Gtk.Button tab_connect_btn;
        private Gtk.Button tab_rules_btn;
        private Gtk.Button tab_log_btn;

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
                h = 580;
            }
            this.set_default_size (w, h);
            this.title = "SShuttle";

            this.setup_actions ();
            this.build_ui ();

            this.tunnel_manager.state_changed.connect (() => {
                this.update_cards_state ();
            });

            this.tunnel_manager.profile_changed.connect (() => {
                this.update_cards_state ();
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
        }

        private void build_ui () {
            var toolbar_view = new Adw.ToolbarView ();
            this.set_content (toolbar_view);

            // 极简原生窗体顶栏
            var header_bar = new Adw.HeaderBar ();
            header_bar.show_title = true;
            toolbar_view.add_top_bar (header_bar);

            // 主垂直容器：顶部独立导航栏 + 分割线 + ViewStack 内容区
            var main_vbox = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            toolbar_view.set_content (main_vbox);

            // 独立的整齐导航栏 (参考截图设计)
            var nav_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 12);
            nav_bar.margin_start = 20;
            nav_bar.margin_end = 20;
            nav_bar.margin_top = 10;
            nav_bar.margin_bottom = 10;
            main_vbox.append (nav_bar);

            // 左侧 Tab 按钮组 (Connect / Rules / Log)
            var tabs_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            nav_bar.append (tabs_box);

            this.tab_connect_btn = this.create_tab_button ("Connect", "connect");
            this.tab_rules_btn = this.create_tab_button ("Rules", "rules");
            this.tab_log_btn = this.create_tab_button ("Log", "log");

            tabs_box.append (this.tab_connect_btn);
            tabs_box.append (this.tab_rules_btn);
            tabs_box.append (this.tab_log_btn);

            // 弹簧占位
            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            nav_bar.append (spacer);

            // 右侧操作项：+ Add Connection 按钮与 Settings 菜单
            var right_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            nav_bar.append (right_box);

            var add_btn = new Gtk.Button ();
            add_btn.add_css_class ("suggested-action");

            var add_content = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            var add_icon = new Gtk.Image.from_icon_name ("list-add-symbolic");
            var add_label = new Gtk.Label ("Add Connection");
            add_content.append (add_icon);
            add_content.append (add_label);
            add_btn.set_child (add_content);
            add_btn.clicked.connect (this.on_add_profile);
            right_box.append (add_btn);

            var settings_btn = new Gtk.MenuButton ();
            settings_btn.icon_name = "emblem-system-symbolic";
            settings_btn.tooltip_text = "Settings";

            var menu = new GLib.Menu ();
            menu.append ("About SShuttle", "app.about");
            menu.append ("Quit", "app.quit");
            settings_btn.menu_model = menu;
            right_box.append (settings_btn);

            // 导航栏下方精细分割线
            var nav_separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            main_vbox.append (nav_separator);

            // ViewStack 内容承载区
            this.view_stack = new Adw.ViewStack ();
            this.view_stack.vexpand = true;
            main_vbox.append (this.view_stack);

            // Page 1: Connect 视图
            var connect_page = this.build_connect_page ();
            this.view_stack.add_named (connect_page, "connect");

            // Page 2: Rules 视图
            var rules_view = new RulesView (this.config_manager, this.tunnel_manager);
            this.view_stack.add_named (rules_view, "rules");

            // Page 3: Log 视图
            var log_view = new LogView (this.tunnel_manager);
            this.view_stack.add_named (log_view, "log");

            // 默认选中 Connect Tab
            this.switch_to_tab ("connect");

            this.refresh_connections ();
        }

        private Gtk.Button create_tab_button (string label, string page_name) {
            var btn = new Gtk.Button.with_label (label);
            btn.add_css_class ("flat");
            btn.add_css_class ("title-4");
            btn.clicked.connect (() => {
                this.switch_to_tab (page_name);
            });
            return btn;
        }

        private void switch_to_tab (string page_name) {
            this.view_stack.visible_child_name = page_name;

            this.tab_connect_btn.remove_css_class ("suggested-action");
            this.tab_rules_btn.remove_css_class ("suggested-action");
            this.tab_log_btn.remove_css_class ("suggested-action");

            if (page_name == "connect") {
                this.tab_connect_btn.add_css_class ("suggested-action");
            } else if (page_name == "rules") {
                this.tab_rules_btn.add_css_class ("suggested-action");
            } else if (page_name == "log") {
                this.tab_log_btn.add_css_class ("suggested-action");
            }
        }

        private Gtk.Widget build_connect_page () {
            this.content_stack = new Gtk.Stack ();

            // 空状态占位页
            this.empty_page = new Adw.StatusPage ();
            this.empty_page.icon_name = "network-workgroup-symbolic";
            this.empty_page.title = "No Connections";
            this.empty_page.description = "Click 'Add Connection' above to add your first server.";
            this.content_stack.add_named (this.empty_page, "empty");

            // 多卡片 FlowBox 容器
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;

            this.flow_box = new Gtk.FlowBox ();
            this.flow_box.valign = Gtk.Align.START;
            this.flow_box.max_children_per_line = 10;
            this.flow_box.min_children_per_line = 1;
            this.flow_box.selection_mode = Gtk.SelectionMode.NONE;
            this.flow_box.margin_start = 20;
            this.flow_box.margin_end = 20;
            this.flow_box.margin_top = 16;
            this.flow_box.margin_bottom = 20;
            this.flow_box.column_spacing = 16;
            this.flow_box.row_spacing = 16;

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
            var editor = new ProfileEditorWindow (null, this);
            editor.profile_saved.connect ((new_p) => {
                this.config_manager.save_profile (new_p);
                this.refresh_connections ();
            });
            editor.present ();
        }

        private void on_edit_profile (Profile profile) {
            var editor = new ProfileEditorWindow (profile, this);
            editor.profile_saved.connect ((saved_p) => {
                this.config_manager.save_profile (saved_p);
                this.refresh_connections ();
            });
            editor.profile_deleted.connect ((deleted_p) => {
                this.config_manager.delete_profile (deleted_p.id);
                this.refresh_connections ();
            });
            editor.present ();
        }
    }
}

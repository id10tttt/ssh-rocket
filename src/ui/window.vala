namespace Sshuttle {

    public class MainWindow : Adw.ApplicationWindow {
        private TunnelManager tunnel_manager;
        private ConfigManager config_manager;

        private Adw.ViewStack view_stack;
        private Gtk.FlowBox flow_box;
        private Adw.StatusPage empty_page;
        private Gtk.Stack content_stack;
        private GLib.GenericArray<ConnectionCard> cards;

        public MainWindow (Adw.Application app, TunnelManager tunnel_manager) {
            Object (application: app);
            this.tunnel_manager = tunnel_manager;
            this.config_manager = tunnel_manager.config_manager;
            this.cards = new GLib.GenericArray<ConnectionCard> ();

            int w = this.config_manager.get_window_width ();
            int h = this.config_manager.get_window_height ();
            if (w < 780) {
                w = 860;
            }
            if (h < 500) {
                h = 560;
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

            this.close_request.connect (() => {
                int cur_w, cur_h;
                this.get_default_size (out cur_w, out cur_h);
                this.config_manager.set_window_size (cur_w, cur_h);
                return false;
            });
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

            var header_bar = new Adw.HeaderBar ();
            toolbar_view.add_top_bar (header_bar);

            // 核心 ViewStack (Connect, Rules, Log)
            this.view_stack = new Adw.ViewStack ();
            toolbar_view.set_content (this.view_stack);

            // 顶部 ViewSwitcher 居中放置
            var switcher = new Adw.ViewSwitcher ();
            switcher.stack = this.view_stack;
            switcher.policy = Adw.ViewSwitcherPolicy.WIDE;
            header_bar.title_widget = switcher;

            // 右侧快捷操作：Add Connection 按钮
            var add_btn = new Gtk.Button ();
            add_btn.add_css_class ("suggested-action");

            var add_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            var add_icon = new Gtk.Image.from_icon_name ("list-add-symbolic");
            var add_label = new Gtk.Label ("Add Connection");
            add_box.append (add_icon);
            add_box.append (add_label);
            add_btn.set_child (add_box);
            add_btn.clicked.connect (this.on_add_profile);
            header_bar.pack_end (add_btn);

            // 主菜单 (About, Quit)
            var menu = new GLib.Menu ();
            menu.append ("About", "app.about");
            menu.append ("Quit", "app.quit");

            var menu_btn = new Gtk.MenuButton ();
            menu_btn.icon_name = "open-menu-symbolic";
            menu_btn.menu_model = menu;
            header_bar.pack_end (menu_btn);

            // Page 1: Connect 视图
            var connect_page = this.build_connect_page ();
            var connect_stack_page = this.view_stack.add_titled (connect_page, "connect", "Connect");
            connect_stack_page.icon_name = "network-vpn-symbolic";

            // Page 2: Rules 视图
            var rules_view = new RulesView (this.config_manager, this.tunnel_manager);
            var rules_stack_page = this.view_stack.add_titled (rules_view, "rules", "Rules");
            rules_stack_page.icon_name = "preferences-system-network-symbolic";

            // Page 3: Log 视图
            var log_view = new LogView (this.tunnel_manager);
            var log_stack_page = this.view_stack.add_titled (log_view, "log", "Log");
            log_stack_page.icon_name = "utilities-terminal-symbolic";

            this.refresh_connections ();
        }

        private Gtk.Widget build_connect_page () {
            this.content_stack = new Gtk.Stack ();

            // 空状态占位页
            this.empty_page = new Adw.StatusPage ();
            this.empty_page.icon_name = "network-workgroup-symbolic";
            this.empty_page.title = "No Connections";
            this.empty_page.description = "Add a connection to start sshuttle proxy.";

            var empty_add_btn = new Gtk.Button.with_label ("Add Connection");
            empty_add_btn.add_css_class ("suggested-action");
            empty_add_btn.add_css_class ("pill");
            empty_add_btn.halign = Gtk.Align.CENTER;
            empty_add_btn.clicked.connect (this.on_add_profile);
            this.empty_page.child = empty_add_btn;

            this.content_stack.add_named (this.empty_page, "empty");

            // 多卡片 FlowBox 容器
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;

            this.flow_box = new Gtk.FlowBox ();
            this.flow_box.valign = Gtk.Align.START;
            this.flow_box.max_children_per_line = 10;
            this.flow_box.min_children_per_line = 1;
            this.flow_box.selection_mode = Gtk.SelectionMode.NONE;
            this.flow_box.margin_start = 16;
            this.flow_box.margin_end = 16;
            this.flow_box.margin_top = 16;
            this.flow_box.margin_bottom = 16;
            this.flow_box.column_spacing = 14;
            this.flow_box.row_spacing = 14;

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

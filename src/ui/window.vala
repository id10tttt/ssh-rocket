namespace Sshuttle {

    public class MainWindow : Adw.ApplicationWindow {
        private TunnelManager tunnel_manager;
        private ConfigManager config_manager;

        private StatusCard status_card;
        private Adw.PreferencesGroup profiles_group;

        public MainWindow (Adw.Application app, TunnelManager tunnel_manager) {
            Object (application: app);
            this.tunnel_manager = tunnel_manager;
            this.config_manager = tunnel_manager.config_manager;

            int w = this.config_manager.get_window_width ();
            int h = this.config_manager.get_window_height ();
            this.set_default_size (w, h);
            this.title = "SShuttle";

            this.setup_actions ();
            this.build_ui ();

            this.tunnel_manager.profile_changed.connect (() => {
                this.refresh_profiles_list ();
            });

            this.close_request.connect (() => {
                int cur_w, cur_h;
                this.get_default_size (out cur_w, out cur_h);
                this.config_manager.set_window_size (cur_w, cur_h);
                return false;
            });
        }

        private void setup_actions () {
            var show_logs_action = new GLib.SimpleAction ("show-logs", null);
            show_logs_action.activate.connect (() => {
                this.show_logs ();
            });
            this.add_action (show_logs_action);

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

            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.tooltip_text = "Add Profile";
            add_btn.clicked.connect (this.on_add_profile);
            header_bar.pack_start (add_btn);

            var menu = new GLib.Menu ();
            menu.append ("Logs", "win.show-logs");
            menu.append ("About", "app.about");

            var menu_btn = new Gtk.MenuButton ();
            menu_btn.icon_name = "open-menu-symbolic";
            menu_btn.menu_model = menu;
            header_bar.pack_end (menu_btn);

            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            toolbar_view.set_content (scrolled);

            var clamp = new Adw.Clamp ();
            clamp.maximum_size = 520;
            clamp.tightening_threshold = 380;
            scrolled.set_child (clamp);

            var content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 16);
            content_box.margin_top = 16;
            content_box.margin_bottom = 24;
            clamp.set_child (content_box);

            this.status_card = new StatusCard (this.tunnel_manager);
            content_box.append (this.status_card);

            this.profiles_group = new Adw.PreferencesGroup ();
            this.profiles_group.title = "Profiles";
            this.profiles_group.margin_start = 16;
            this.profiles_group.margin_end = 16;
            content_box.append (this.profiles_group);

            this.refresh_profiles_list ();
        }

        public void refresh_profiles_list () {
            var child = this.profiles_group.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.profiles_group.remove (child);
                child = next;
            }

            var profiles = this.config_manager.get_profiles ();
            var active = this.config_manager.get_active_profile ();
            string? active_id = (active != null) ? active.id : null;

            foreach (var p in profiles) {
                bool is_active = (active_id != null && p.id == active_id);
                var row = new ProfileRow (p, is_active);

                row.activated_profile.connect ((selected) => {
                    this.tunnel_manager.set_active_profile (selected.id);
                });

                row.edit_clicked.connect ((editing) => {
                    this.on_edit_profile (editing);
                });

                this.profiles_group.add (row);
            }

            this.status_card.update_view ();
        }

        private void on_add_profile () {
            var editor = new ProfileEditorWindow (null, this);
            editor.profile_saved.connect ((new_p) => {
                this.config_manager.save_profile (new_p);
                this.refresh_profiles_list ();
            });
            editor.present ();
        }

        private void on_edit_profile (Profile profile) {
            var editor = new ProfileEditorWindow (profile, this);
            editor.profile_saved.connect ((saved_p) => {
                this.config_manager.save_profile (saved_p);
                this.refresh_profiles_list ();
            });
            editor.profile_deleted.connect ((deleted_p) => {
                this.config_manager.delete_profile (deleted_p.id);
                this.refresh_profiles_list ();
            });
            editor.present ();
        }

        public void show_logs () {
            var log_win = new LogWindow (this.tunnel_manager, this);
            log_win.present ();
        }
    }
}

namespace Sshuttle {

    public class StatusCard : Gtk.Box {
        private TunnelManager tunnel_manager;

        private Gtk.Image status_icon;
        private Gtk.Label status_label;
        private Gtk.Label profile_name_label;
        private Gtk.Label summary_label;
        private Gtk.Button action_button;
        private Gtk.Label btn_label;
        private Gtk.Spinner spinner;

        public StatusCard (TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 12);
            this.tunnel_manager = tunnel_manager;

            this.add_css_class ("card");
            this.margin_top = 12;
            this.margin_bottom = 12;
            this.margin_start = 16;
            this.margin_end = 16;

            var inner_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 10);
            inner_box.margin_top = 16;
            inner_box.margin_bottom = 16;
            inner_box.margin_start = 16;
            inner_box.margin_end = 16;
            this.append (inner_box);

            var status_row = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            this.status_icon = new Gtk.Image.from_icon_name ("media-record-symbolic");
            this.status_icon.pixel_size = 12;
            status_row.append (this.status_icon);

            this.status_label = new Gtk.Label ("");
            this.status_label.add_css_class ("title-4");
            status_row.append (this.status_label);
            inner_box.append (status_row);

            var info_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            this.profile_name_label = new Gtk.Label ("");
            this.profile_name_label.xalign = 0;
            this.profile_name_label.add_css_class ("title-2");
            info_box.append (this.profile_name_label);

            this.summary_label = new Gtk.Label ("");
            this.summary_label.xalign = 0;
            this.summary_label.add_css_class ("dim-label");
            info_box.append (this.summary_label);
            inner_box.append (info_box);

            this.action_button = new Gtk.Button ();
            this.action_button.hexpand = true;
            this.action_button.add_css_class ("pill");
            this.action_button.clicked.connect (() => {
                this.tunnel_manager.toggle_connection ();
            });

            this.spinner = new Gtk.Spinner ();
            this.spinner.visible = false;

            var btn_content = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            btn_content.halign = Gtk.Align.CENTER;
            this.btn_label = new Gtk.Label ("");
            this.btn_label.add_css_class ("title-4");

            btn_content.append (this.spinner);
            btn_content.append (this.btn_label);
            this.action_button.set_child (btn_content);

            inner_box.append (this.action_button);

            this.tunnel_manager.state_changed.connect (() => {
                this.update_view ();
            });
            this.tunnel_manager.profile_changed.connect (() => {
                this.update_view ();
            });

            this.update_view ();
        }

        public void update_view () {
            var state = this.tunnel_manager.state;
            var profile = this.tunnel_manager.active_profile;

            this.action_button.remove_css_class ("suggested-action");
            this.action_button.remove_css_class ("destructive-action");
            this.status_icon.remove_css_class ("success");
            this.status_icon.remove_css_class ("warning");
            this.status_icon.remove_css_class ("error");
            this.status_icon.remove_css_class ("dim-label");

            if (profile != null) {
                this.profile_name_label.label = profile.name;
                this.summary_label.label = profile.get_summary ();
            } else {
                this.profile_name_label.label = "No Profile";
                this.summary_label.label = "";
            }

            switch (state) {
                case TunnelState.CONNECTED:
                    this.status_label.label = "Connected";
                    this.status_icon.add_css_class ("success");
                    this.btn_label.label = "Disconnect";
                    this.action_button.add_css_class ("destructive-action");
                    this.action_button.sensitive = true;
                    this.spinner.visible = false;
                    this.spinner.stop ();
                    break;

                case TunnelState.CONNECTING:
                    this.status_label.label = "Connecting";
                    this.status_icon.add_css_class ("warning");
                    this.btn_label.label = "Connecting";
                    this.action_button.sensitive = false;
                    this.spinner.visible = true;
                    this.spinner.start ();
                    break;

                case TunnelState.DISCONNECTING:
                    this.status_label.label = "Disconnecting";
                    this.status_icon.add_css_class ("warning");
                    this.btn_label.label = "Disconnecting";
                    this.action_button.sensitive = false;
                    this.spinner.visible = true;
                    this.spinner.start ();
                    break;

                case TunnelState.ERROR:
                    this.status_label.label = "Error";
                    this.status_icon.add_css_class ("error");
                    this.btn_label.label = "Reconnect";
                    this.action_button.add_css_class ("suggested-action");
                    this.action_button.sensitive = (profile != null);
                    this.spinner.visible = false;
                    this.spinner.stop ();
                    break;

                default: // DISCONNECTED
                    this.status_label.label = "Disconnected";
                    this.status_icon.add_css_class ("dim-label");
                    this.btn_label.label = "Connect";
                    this.action_button.add_css_class ("suggested-action");
                    this.action_button.sensitive = (profile != null);
                    this.spinner.visible = false;
                    this.spinner.stop ();
                    break;
            }
        }
    }
}

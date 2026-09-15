namespace Sshuttle {

    public class ConnectionCard : Gtk.Box {
        public Profile profile { get; private set; }

        public signal void connect_requested (Profile profile);
        public signal void disconnect_requested ();
        public signal void edit_requested (Profile profile);
        public signal void delete_requested (Profile profile);

        private TunnelManager tunnel_manager;
        private Gtk.Button action_btn;
        private Gtk.Label btn_label;
        private Gtk.Spinner spinner;
        private Gtk.Image status_icon;
        private Gtk.Label status_label;

        public ConnectionCard (Profile profile, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.profile = profile;
            this.tunnel_manager = tunnel_manager;

            this.add_css_class ("card");
            this.set_size_request (300, -1);

            // 内部主体布局
            var inner_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            inner_box.margin_start = 16;
            inner_box.margin_end = 16;
            inner_box.margin_top = 16;
            inner_box.margin_bottom = 12;
            this.append (inner_box);

            // 顶部：状态图标 + 节点名称 + 编辑/删除操作
            var header_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 10);
            header_box.margin_bottom = 12;

            this.status_icon = new Gtk.Image.from_icon_name ("ssh-rocket-disconnected-symbolic");
            this.status_icon.pixel_size = 24;
            this.status_icon.add_css_class ("dim-label");
            header_box.append (this.status_icon);

            var title_label = new Gtk.Label (profile.name);
            title_label.add_css_class ("title-3");
            title_label.xalign = 0;
            title_label.hexpand = true;
            title_label.ellipsize = Pango.EllipsizeMode.END;
            header_box.append (title_label);

            var edit_btn = new Gtk.Button.from_icon_name ("document-edit-symbolic");
            edit_btn.add_css_class ("flat");
            edit_btn.add_css_class ("dim-label");
            edit_btn.valign = Gtk.Align.CENTER;
            edit_btn.tooltip_text = "Edit";
            edit_btn.clicked.connect (() => {
                this.edit_requested (this.profile);
            });
            header_box.append (edit_btn);

            var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
            del_btn.add_css_class ("flat");
            del_btn.add_css_class ("dim-label");
            del_btn.valign = Gtk.Align.CENTER;
            del_btn.tooltip_text = "Delete";
            del_btn.clicked.connect (() => {
                this.delete_requested (this.profile);
            });
            header_box.append (del_btn);

            inner_box.append (header_box);

            // 中间信息区
            var info_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 6);
            info_box.margin_bottom = 12;
            inner_box.append (info_box);

            this.add_info_row (info_box, "Server", profile.host);
            this.add_info_row (info_box, "Port", @"$(profile.port)");
            string user_text = (profile.username != "") ? profile.username : "—";
            this.add_info_row (info_box, "Username", user_text);
            this.add_info_row (info_box, "Login Mode", profile.get_login_mode_label ());

            // 底部：状态文字 + 主操作按钮
            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.margin_bottom = 10;
            inner_box.append (separator);

            var bottom_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            inner_box.append (bottom_bar);

            this.status_label = new Gtk.Label ("Disconnected");
            this.status_label.add_css_class ("dim-label");
            this.status_label.add_css_class ("caption");
            this.status_label.xalign = 0;
            this.status_label.hexpand = true;
            bottom_bar.append (this.status_label);

            // 主操作按钮
            this.action_btn = new Gtk.Button ();
            this.action_btn.add_css_class ("pill");

            var btn_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            btn_box.halign = Gtk.Align.CENTER;

            this.spinner = new Gtk.Spinner ();
            this.spinner.visible = false;
            btn_box.append (this.spinner);

            this.btn_label = new Gtk.Label ("Connect");
            btn_box.append (this.btn_label);

            this.action_btn.set_child (btn_box);
            this.action_btn.clicked.connect (this.on_action_clicked);
            bottom_bar.append (this.action_btn);

            this.update_state ();
        }

        private void add_info_row (Gtk.Box parent, string key, string val) {
            var row = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);

            var key_lbl = new Gtk.Label (key);
            key_lbl.xalign = 0;
            key_lbl.add_css_class ("dim-label");
            key_lbl.add_css_class ("caption");
            row.append (key_lbl);

            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            row.append (spacer);

            var val_lbl = new Gtk.Label (val);
            val_lbl.xalign = 1;
            val_lbl.add_css_class ("caption");
            val_lbl.ellipsize = Pango.EllipsizeMode.END;
            val_lbl.max_width_chars = 22;
            row.append (val_lbl);

            parent.append (row);
        }

        public void update_state () {
            var active_p = this.tunnel_manager.active_profile;
            bool is_active = (active_p != null && active_p.id == this.profile.id);
            var state = this.tunnel_manager.state;

            this.action_btn.remove_css_class ("suggested-action");
            this.action_btn.remove_css_class ("destructive-action");

            // 重置状态图标
            this.status_icon.remove_css_class ("accent");
            this.status_icon.remove_css_class ("success");
            this.status_icon.remove_css_class ("warning");
            this.status_icon.remove_css_class ("error");
            this.status_icon.remove_css_class ("dim-label");

            if (is_active) {
                switch (state) {
                    case TunnelState.CONNECTED:
                        this.btn_label.label = "Disconnect";
                        this.action_btn.add_css_class ("destructive-action");
                        this.action_btn.sensitive = true;
                        this.spinner.visible = false;
                        this.spinner.stop ();
                        this.status_icon.icon_name = "ssh-rocket-symbolic";
                        this.status_icon.add_css_class ("success");
                        this.status_label.label = "Connected";
                        break;

                    case TunnelState.CONNECTING:
                        this.btn_label.label = "Connecting";
                        this.action_btn.sensitive = false;
                        this.spinner.visible = true;
                        this.spinner.start ();
                        this.status_icon.icon_name = "ssh-rocket-acquiring-symbolic";
                        this.status_icon.add_css_class ("warning");
                        this.status_label.label = "Connecting…";
                        break;

                    case TunnelState.DISCONNECTING:
                        this.btn_label.label = "Stopping";
                        this.action_btn.sensitive = false;
                        this.spinner.visible = true;
                        this.spinner.start ();
                        this.status_icon.icon_name = "ssh-rocket-acquiring-symbolic";
                        this.status_icon.add_css_class ("warning");
                        this.status_label.label = "Disconnecting…";
                        break;

                    case TunnelState.ERROR:
                        if (this.tunnel_manager.reconnect_attempt > 0) {
                            this.btn_label.label = "Stop Retry";
                            this.action_btn.add_css_class ("destructive-action");
                            this.action_btn.sensitive = true;
                            this.spinner.visible = true;
                            this.spinner.start ();
                            this.status_icon.icon_name = "ssh-rocket-acquiring-symbolic";
                            this.status_icon.add_css_class ("warning");
                            this.status_label.label = @"Reconnecting (#$(this.tunnel_manager.reconnect_attempt))…";
                        } else {
                            this.btn_label.label = "Reconnect";
                            this.action_btn.add_css_class ("suggested-action");
                            this.action_btn.sensitive = true;
                            this.spinner.visible = false;
                            this.spinner.stop ();
                            this.status_icon.icon_name = "dialog-error-symbolic";
                            this.status_icon.add_css_class ("error");
                            this.status_label.label = "Error";
                        }
                        break;

                    default: // DISCONNECTED
                        this.btn_label.label = "Connect";
                        this.action_btn.add_css_class ("suggested-action");
                        this.action_btn.sensitive = true;
                        this.spinner.visible = false;
                        this.spinner.stop ();
                        this.status_icon.icon_name = "ssh-rocket-disconnected-symbolic";
                        this.status_icon.add_css_class ("dim-label");
                        this.status_label.label = "Disconnected";
                        break;
                }
            } else {
                this.btn_label.label = "Connect";
                this.action_btn.add_css_class ("suggested-action");
                this.action_btn.sensitive = (state != TunnelState.CONNECTING && state != TunnelState.DISCONNECTING);
                this.spinner.visible = false;
                this.spinner.stop ();
                this.status_icon.icon_name = "ssh-rocket-disconnected-symbolic";
                this.status_icon.add_css_class ("dim-label");
                this.status_label.label = "Disconnected";
            }
        }

        private void on_action_clicked () {
            var active_p = this.tunnel_manager.active_profile;
            bool is_active = (active_p != null && active_p.id == this.profile.id);
            var state = this.tunnel_manager.state;

            if (is_active && (state == TunnelState.CONNECTED || state == TunnelState.CONNECTING || (state == TunnelState.ERROR && this.tunnel_manager.reconnect_attempt > 0))) {
                this.disconnect_requested ();
            } else {
                this.connect_requested (this.profile);
            }
        }
    }
}

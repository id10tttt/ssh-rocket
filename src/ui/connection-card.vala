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
        private Gtk.Image status_dot;

        public ConnectionCard (Profile profile, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 10);
            this.profile = profile;
            this.tunnel_manager = tunnel_manager;

            this.add_css_class ("card");
            this.set_size_request (280, -1);
            this.margin_start = 8;
            this.margin_end = 8;
            this.margin_top = 8;
            this.margin_bottom = 8;

            // 内部主体布局
            var inner_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 10);
            inner_box.margin_start = 14;
            inner_box.margin_end = 14;
            inner_box.margin_top = 14;
            inner_box.margin_bottom = 14;
            this.append (inner_box);

            // 顶部：图标 + 节点名称 + 状态小圆点
            var header_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 10);
            var icon = new Gtk.Image.from_icon_name ("network-workgroup-symbolic");
            icon.pixel_size = 24;
            icon.add_css_class ("accent");
            header_box.append (icon);

            var title_label = new Gtk.Label (profile.name);
            title_label.add_css_class ("title-3");
            title_label.xalign = 0;
            title_label.hexpand = true;
            title_label.ellipsize = Pango.EllipsizeMode.END;
            header_box.append (title_label);

            this.status_dot = new Gtk.Image.from_icon_name ("media-record-symbolic");
            this.status_dot.pixel_size = 10;
            this.status_dot.visible = false;
            header_box.append (this.status_dot);

            inner_box.append (header_box);

            // 中间信息区 (类似截图排版)
            var grid = new Gtk.Grid ();
            grid.row_spacing = 6;
            grid.column_spacing = 16;
            inner_box.append (grid);

            this.add_info_row (grid, 0, "Server:", profile.host);
            this.add_info_row (grid, 1, "Port:", @"$(profile.port)");
            string user_text = (profile.username != "") ? profile.username : "None";
            this.add_info_row (grid, 2, "Username:", user_text);
            this.add_info_row (grid, 3, "Routing:", profile.get_summary ());

            // 底部分割线
            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.margin_top = 4;
            separator.margin_bottom = 2;
            inner_box.append (separator);

            // 底部操作区：左侧编辑、删除；右侧 Connect/Disconnect 按钮
            var bottom_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            inner_box.append (bottom_bar);

            var edit_btn = new Gtk.Button.from_icon_name ("document-edit-symbolic");
            edit_btn.add_css_class ("flat");
            edit_btn.tooltip_text = "Edit";
            edit_btn.clicked.connect (() => {
                this.edit_requested (this.profile);
            });
            bottom_bar.append (edit_btn);

            var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
            del_btn.add_css_class ("flat");
            del_btn.tooltip_text = "Delete";
            del_btn.clicked.connect (() => {
                this.delete_requested (this.profile);
            });
            bottom_bar.append (del_btn);

            // 弹簧占位
            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            bottom_bar.append (spacer);

            // 右侧主操作按钮
            this.action_btn = new Gtk.Button ();
            this.action_btn.add_css_class ("pill");
            this.action_btn.set_size_request (95, -1);

            var btn_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 6);
            btn_box.halign = Gtk.Align.CENTER;

            this.spinner = new Gtk.Spinner ();
            this.spinner.visible = false;
            btn_box.append (this.spinner);

            this.btn_label = new Gtk.Label ("Connect");
            this.btn_label.add_css_class ("title-4");
            btn_box.append (this.btn_label);

            this.action_btn.set_child (btn_box);
            this.action_btn.clicked.connect (this.on_action_clicked);
            bottom_bar.append (this.action_btn);

            this.update_state ();
        }

        private void add_info_row (Gtk.Grid grid, int row, string key, string val) {
            var key_lbl = new Gtk.Label (key);
            key_lbl.xalign = 0;
            key_lbl.add_css_class ("dim-label");
            grid.attach (key_lbl, 0, row);

            var val_lbl = new Gtk.Label (val);
            val_lbl.xalign = 1;
            val_lbl.hexpand = true;
            val_lbl.ellipsize = Pango.EllipsizeMode.END;
            grid.attach (val_lbl, 1, row);
        }

        public void update_state () {
            var active_p = this.tunnel_manager.active_profile;
            bool is_active = (active_p != null && active_p.id == this.profile.id);
            var state = this.tunnel_manager.state;

            this.action_btn.remove_css_class ("suggested-action");
            this.action_btn.remove_css_class ("destructive-action");
            this.action_btn.remove_css_class ("success");
            this.remove_css_class ("accent-border");
            this.status_dot.visible = false;

            if (is_active) {
                this.status_dot.visible = true;
                this.status_dot.remove_css_class ("success");
                this.status_dot.remove_css_class ("warning");
                this.status_dot.remove_css_class ("error");

                switch (state) {
                    case TunnelState.CONNECTED:
                        this.btn_label.label = "Disconnect";
                        this.action_btn.add_css_class ("destructive-action");
                        this.action_btn.sensitive = true;
                        this.spinner.visible = false;
                        this.spinner.stop ();
                        this.status_dot.add_css_class ("success");
                        break;

                    case TunnelState.CONNECTING:
                        this.btn_label.label = "Connecting";
                        this.action_btn.sensitive = false;
                        this.spinner.visible = true;
                        this.spinner.start ();
                        this.status_dot.add_css_class ("warning");
                        break;

                    case TunnelState.DISCONNECTING:
                        this.btn_label.label = "Stopping";
                        this.action_btn.sensitive = false;
                        this.spinner.visible = true;
                        this.spinner.start ();
                        this.status_dot.add_css_class ("warning");
                        break;

                    case TunnelState.ERROR:
                        this.btn_label.label = "Reconnect";
                        this.action_btn.add_css_class ("suggested-action");
                        this.action_btn.sensitive = true;
                        this.spinner.visible = false;
                        this.spinner.stop ();
                        this.status_dot.add_css_class ("error");
                        break;

                    default: // DISCONNECTED
                        this.btn_label.label = "Connect";
                        this.action_btn.add_css_class ("suggested-action");
                        this.action_btn.sensitive = true;
                        this.spinner.visible = false;
                        this.spinner.stop ();
                        break;
                }
            } else {
                this.btn_label.label = "Connect";
                this.action_btn.add_css_class ("suggested-action");
                // 若其它节点正在连接或已连接，此节点可切换激活连接
                this.action_btn.sensitive = (state != TunnelState.CONNECTING && state != TunnelState.DISCONNECTING);
                this.spinner.visible = false;
                this.spinner.stop ();
            }
        }

        private void on_action_clicked () {
            var active_p = this.tunnel_manager.active_profile;
            bool is_active = (active_p != null && active_p.id == this.profile.id);
            var state = this.tunnel_manager.state;

            if (is_active && (state == TunnelState.CONNECTED || state == TunnelState.CONNECTING)) {
                this.disconnect_requested ();
            } else {
                this.connect_requested (this.profile);
            }
        }
    }
}

namespace Sshuttle {

    public class ProfileEditorWindow : Adw.PreferencesDialog {
        public signal void profile_saved (Profile profile);
        public signal void profile_deleted (Profile profile);

        private Profile? original_profile;
        private bool is_new;

        private Adw.EntryRow name_row;
        private Adw.EntryRow host_row;
        private Adw.SpinRow port_row;
        private Adw.EntryRow user_row;

        private Adw.ComboRow auth_row;
        private Adw.EntryRow key_row;
        private Adw.PasswordEntryRow password_row;

        private Adw.EntryRow routes_row;
        private Adw.SwitchRow dns_row;
        private Adw.SwitchRow ipv6_row;

        private Adw.PreferencesGroup exclude_group;
        private Gtk.Box exclude_rows_box;
        private Adw.EntryRow new_exclude_entry;
        private GLib.GenericArray<string> excludes;

        private Adw.ComboRow verbosity_row;
        private Adw.SwitchRow auto_connect_row;

        private static string[] AUTH_TYPES = { "agent", "key", "password" };
        private static string[] AUTH_LABELS = { "SSH Agent / Default", "Private Key File", "Password" };

        private static string[] VERBOSITIES = { "normal", "verbose", "very_verbose" };
        private static string[] VERBOSITY_LABELS = { "Normal", "Verbose", "Very Verbose" };

        public ProfileEditorWindow (Profile? profile = null) {
            this.original_profile = profile;
            this.is_new = (profile == null);
            this.content_width = 460;
            this.content_height = 660;

            this.excludes = new GLib.GenericArray<string> ();
            if (profile != null) {
                foreach (var exc in profile.exclude) {
                    this.excludes.add (exc);
                }
            }

            this.title = this.is_new ? "New Profile" : "Edit Profile";

            var page = new Adw.PreferencesPage ();
            this.add (page);

            // SSH 分组
            var ssh_group = new Adw.PreferencesGroup ();
            ssh_group.title = "SSH Connection";
            page.add (ssh_group);

            this.name_row = new Adw.EntryRow ();
            this.name_row.title = "Name";
            this.name_row.text = (profile != null) ? profile.name : "New Server";
            ssh_group.add (this.name_row);

            this.host_row = new Adw.EntryRow ();
            this.host_row.title = "Host";
            this.host_row.text = (profile != null) ? profile.host : "";
            ssh_group.add (this.host_row);

            this.port_row = new Adw.SpinRow.with_range (1, 65535, 1);
            this.port_row.title = "Port";
            this.port_row.value = (profile != null) ? profile.port : 22;
            ssh_group.add (this.port_row);

            this.user_row = new Adw.EntryRow ();
            this.user_row.title = "Username";
            this.user_row.text = (profile != null) ? profile.username : "";
            ssh_group.add (this.user_row);

            // 认证模式
            this.auth_row = new Adw.ComboRow ();
            this.auth_row.title = "Login Mode";
            var auth_model = Native.string_list (AUTH_LABELS);
            this.auth_row.model = auth_model;
            string cur_auth = (profile != null) ? profile.auth_type : "agent";
            for (uint i = 0; i < AUTH_TYPES.length; i++) {
                if (AUTH_TYPES[i] == cur_auth) {
                    this.auth_row.selected = i;
                    break;
                }
            }
            ssh_group.add (this.auth_row);

            // 私钥文件选择行
            this.key_row = new Adw.EntryRow ();
            this.key_row.title = "Private Key File";
            this.key_row.text = (profile != null) ? profile.key_path : "";

            var browse_btn = new Gtk.Button.from_icon_name ("folder-open-symbolic");
            browse_btn.add_css_class ("flat");
            browse_btn.valign = Gtk.Align.CENTER;
            browse_btn.tooltip_text = "Choose Key File";
            browse_btn.clicked.connect (this.on_browse_key_clicked);
            this.key_row.add_suffix (browse_btn);
            ssh_group.add (this.key_row);

            // 密码输入行
            this.password_row = new Adw.PasswordEntryRow ();
            this.password_row.title = "Password";
            this.password_row.text = (profile != null) ? profile.password : "";
            ssh_group.add (this.password_row);

            this.auth_row.notify["selected"].connect (this.update_auth_fields_visibility);
            this.update_auth_fields_visibility ();

            // 路由分组
            var routing_group = new Adw.PreferencesGroup ();
            routing_group.title = "Routing";
            page.add (routing_group);

            this.routes_row = new Adw.EntryRow ();
            this.routes_row.title = "Remote Routes";
            string routes_text = (profile != null && profile.routes.length > 0)
                ? string.joinv (", ", profile.routes)
                : "0.0.0.0/0";
            this.routes_row.text = routes_text;
            routing_group.add (this.routes_row);

            this.dns_row = new Adw.SwitchRow ();
            this.dns_row.title = "DNS Forwarding";
            this.dns_row.active = (profile != null) ? profile.dns : true;
            routing_group.add (this.dns_row);

            this.ipv6_row = new Adw.SwitchRow ();
            this.ipv6_row.title = "IPv6";
            this.ipv6_row.active = (profile != null) ? profile.ipv6 : false;
            routing_group.add (this.ipv6_row);

            // 排除网络分组
            this.exclude_group = new Adw.PreferencesGroup ();
            this.exclude_group.title = "Exclude Networks";
            page.add (this.exclude_group);

            this.exclude_rows_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 6);
            this.exclude_group.add (this.exclude_rows_box);

            this.new_exclude_entry = new Adw.EntryRow ();
            this.new_exclude_entry.title = "Add Network";
            var add_btn = new Gtk.Button.from_icon_name ("list-add-symbolic");
            add_btn.add_css_class ("flat");
            add_btn.valign = Gtk.Align.CENTER;
            add_btn.clicked.connect (this.on_add_exclude);
            this.new_exclude_entry.add_suffix (add_btn);
            this.new_exclude_entry.entry_activated.connect (this.on_add_exclude);
            this.exclude_group.add (this.new_exclude_entry);

            this.refresh_excludes ();

            // 高级选项
            var adv_group = new Adw.PreferencesGroup ();
            adv_group.title = "Advanced";
            page.add (adv_group);

            this.verbosity_row = new Adw.ComboRow ();
            this.verbosity_row.title = "Verbosity";
            var verb_model = Native.string_list (VERBOSITY_LABELS);
            this.verbosity_row.model = verb_model;
            string cur_verb = (profile != null) ? profile.verbosity : "normal";
            for (uint i = 0; i < VERBOSITIES.length; i++) {
                if (VERBOSITIES[i] == cur_verb) {
                    this.verbosity_row.selected = i;
                    break;
                }
            }
            adv_group.add (this.verbosity_row);

            this.auto_connect_row = new Adw.SwitchRow ();
            this.auto_connect_row.title = "Auto Connect";
            this.auto_connect_row.active = (profile != null) ? profile.auto_connect : false;
            adv_group.add (this.auto_connect_row);

            // 操作按钮
            var actions_group = new Adw.PreferencesGroup ();
            page.add (actions_group);

            var save_btn = new Gtk.Button.with_label ("Save");
            save_btn.add_css_class ("suggested-action");
            save_btn.add_css_class ("pill");
            save_btn.margin_top = 8;
            save_btn.margin_bottom = 8;
            save_btn.clicked.connect (this.on_save_clicked);
            actions_group.add (save_btn);

            if (!this.is_new) {
                var del_btn = new Gtk.Button.with_label ("Delete Profile");
                del_btn.add_css_class ("destructive-action");
                del_btn.add_css_class ("pill");
                del_btn.margin_bottom = 12;
                del_btn.clicked.connect (this.on_delete_clicked);
                actions_group.add (del_btn);
            }
        }

        private void update_auth_fields_visibility () {
            uint idx = this.auth_row.selected;
            string auth_mode = (idx < AUTH_TYPES.length) ? AUTH_TYPES[idx] : "agent";

            this.key_row.visible = (auth_mode == "key");
            this.password_row.visible = (auth_mode == "password");
        }

        private void on_browse_key_clicked () {
            var dialog = new Gtk.FileDialog ();
            dialog.title = "Select SSH Private Key";

            dialog.open.begin (this.get_root () as Gtk.Window, null, (obj, res) => {
                try {
                    var file = dialog.open.end (res);
                    if (file != null) {
                        this.key_row.text = file.get_path ();
                    }
                } catch (GLib.Error e) {
                    // 用户取消选择
                }
            });
        }

        private void refresh_excludes () {
            var child = this.exclude_rows_box.get_first_child ();
            while (child != null) {
                var next = child.get_next_sibling ();
                this.exclude_rows_box.remove (child);
                child = next;
            }

            for (uint i = 0; i < this.excludes.length; i++) {
                string item = this.excludes[i];
                var row = new Adw.ActionRow ();
                row.title = item;

                var del_btn = new Gtk.Button.from_icon_name ("user-trash-symbolic");
                del_btn.add_css_class ("flat");
                del_btn.valign = Gtk.Align.CENTER;
                del_btn.clicked.connect (() => {
                    for (uint j = 0; j < this.excludes.length; j++) {
                        if (this.excludes[j] == item) {
                            this.excludes.remove_index (j);
                            break;
                        }
                    }
                    this.refresh_excludes ();
                });
                row.add_suffix (del_btn);
                this.exclude_rows_box.append (row);
            }
        }

        private void on_add_exclude () {
            string text = this.new_exclude_entry.text.strip ();
            if (text != "") {
                bool exists = false;
                for (uint i = 0; i < this.excludes.length; i++) {
                    if (this.excludes[i] == text) {
                        exists = true;
                        break;
                    }
                }
                if (!exists) {
                    this.excludes.add (text);
                    this.new_exclude_entry.text = "";
                    this.refresh_excludes ();
                }
            }
        }

        private void on_save_clicked () {
            if (this.host_row.text.strip () == "") {
                this.show_validation_error ("Host is required.");
                return;
            }

            uint auth_idx = this.auth_row.selected;
            string auth_mode = (auth_idx < AUTH_TYPES.length) ? AUTH_TYPES[auth_idx] : "agent";
            if (auth_mode == "key" && this.key_row.text.strip () == "") {
                this.show_validation_error ("Select a private key file.");
                return;
            }
            if (auth_mode == "password" && this.password_row.text == "") {
                this.show_validation_error ("Password is required for password login.");
                return;
            }

            var p = new Profile ();
            if (this.original_profile != null) {
                p.id = this.original_profile.id;
            }

            p.name = (this.name_row.text.strip () != "") ? this.name_row.text.strip () : "Unnamed";
            p.host = this.host_row.text.strip ();
            p.port = (int) this.port_row.value;
            p.username = this.user_row.text.strip ();

            p.auth_type = auth_mode;
            p.key_path = this.key_row.text.strip ();
            p.password = this.password_row.text;

            string raw_routes = this.routes_row.text.strip ();
            string[] split_routes = raw_routes.split (",");
            var r_list = new GLib.GenericArray<string> ();
            foreach (var r in split_routes) {
                string trimmed = r.strip ();
                if (trimmed != "") {
                    if (!this.is_valid_network (trimmed)) {
                        this.show_validation_error (@"Invalid remote route: $(trimmed)");
                        return;
                    }
                    r_list.add (trimmed);
                }
            }
            if (r_list.length == 0) {
                p.routes = new string[] { "0.0.0.0/0" };
            } else {
                var r_arr = new string[r_list.length];
                for (uint i = 0; i < r_list.length; i++) {
                    r_arr[i] = r_list[i];
                }
                p.routes = r_arr;
            }

            var exc_arr = new string[this.excludes.length];
            for (uint i = 0; i < this.excludes.length; i++) {
                if (!this.is_valid_network (this.excludes[i])) {
                    this.show_validation_error (@"Invalid exclude network: $(this.excludes[i])");
                    return;
                }
                exc_arr[i] = this.excludes[i];
            }
            p.exclude = exc_arr;

            p.dns = this.dns_row.active;
            p.ipv6 = this.ipv6_row.active;

            p.method = "nft";

            uint v_idx = this.verbosity_row.selected;
            p.verbosity = (v_idx < VERBOSITIES.length) ? VERBOSITIES[v_idx] : "normal";

            p.auto_connect = this.auto_connect_row.active;

            this.profile_saved (p);
            this.close ();
        }

        /**
         * 校验 IPv4、IPv6 地址及其 CIDR 前缀。
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

        /**
         * 显示连接配置校验错误。
         */
        private void show_validation_error (string message) {
            var dialog = new Adw.AlertDialog ("Invalid Configuration", message);
            dialog.add_response ("close", "Close");
            dialog.present (this);
        }

        private void on_delete_clicked () {
            if (this.original_profile != null) {
                this.profile_deleted (this.original_profile);
            }
            this.close ();
        }
    }
}

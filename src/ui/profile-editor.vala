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

        private static string[] AUTH_TYPES = { "agent", "key", "password" };
        private static string[] AUTH_LABELS = { "SSH Agent / Default", "Private Key File", "Password" };

        public ProfileEditorWindow (Profile? profile = null) {
            this.original_profile = profile;
            this.is_new = (profile == null);
            this.content_width = 460;
            this.content_height = 520;

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

            this.profile_saved (p);
            this.close ();
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

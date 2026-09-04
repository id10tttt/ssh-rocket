namespace Sshuttle {

    public class ProfileRow : Adw.ActionRow {
        public Profile profile { get; private set; }
        public signal void activated_profile (Profile profile);
        public signal void edit_clicked (Profile profile);

        private Gtk.Image active_icon;

        public ProfileRow (Profile profile, bool is_active) {
            this.profile = profile;

            this.title = profile.name;
            this.subtitle = profile.get_ssh_target ();
            this.activatable = true;

            this.active_icon = new Gtk.Image.from_icon_name ("object-select-symbolic");
            this.active_icon.visible = is_active;
            this.add_prefix (this.active_icon);

            var edit_btn = new Gtk.Button.from_icon_name ("go-next-symbolic");
            edit_btn.add_css_class ("flat");
            edit_btn.valign = Gtk.Align.CENTER;
            edit_btn.clicked.connect (() => {
                this.edit_clicked (this.profile);
            });
            this.add_suffix (edit_btn);

            this.activated.connect (() => {
                this.activated_profile (this.profile);
            });
        }

        public void set_active_state (bool is_active) {
            this.active_icon.visible = is_active;
        }
    }
}

namespace Sshuttle {

    public class Application : Adw.Application {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;
        private MainWindow? window = null;

        public Application () {
            Object (
                application_id: Config.APP_ID,
                flags: GLib.ApplicationFlags.DEFAULT_FLAGS
            );
        }

        public override void startup () {
            base.startup ();

            this.config_manager = new ConfigManager ();
            this.tunnel_manager = new TunnelManager (this.config_manager);

            var quit_action = new GLib.SimpleAction ("quit", null);
            quit_action.activate.connect (() => {
                this.quit ();
            });
            this.add_action (quit_action);

            var about_action = new GLib.SimpleAction ("about", null);
            about_action.activate.connect (() => {
                this.show_about ();
            });
            this.add_action (about_action);

            this.set_accels_for_action ("app.quit", { "<Control>q" });
            this.set_accels_for_action ("win.new-profile", { "<Control>n" });
            this.set_accels_for_action ("win.show-logs", { "<Control>l" });
        }

        public override void activate () {
            base.activate ();

            if (this.window == null) {
                this.window = new MainWindow (this, this.tunnel_manager);
            }
            this.window.present ();
        }

        private void show_about () {
            var about = new Adw.AboutDialog ();
            about.application_name = "SShuttle";
            about.application_icon = "network-vpn-symbolic";
            about.developer_name = "Giggle";
            about.version = Config.VERSION;
            about.copyright = "© 2026 Giggle";
            about.present (this.window);
        }
    }
}

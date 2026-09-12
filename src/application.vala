namespace Sshuttle {

    public class Application : Adw.Application {
        private ConfigManager config_manager;
        private TunnelManager tunnel_manager;
        private TrayManager tray_manager;
        private MainWindow? window = null;
        private bool cleanup_completed = false;

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
            this.tray_manager = new TrayManager (this.config_manager, this.tunnel_manager);

            this.tray_manager.show_window_requested.connect (() => {
                if (this.window != null) {
                    this.window.show_and_present ();
                }
            });

            this.tray_manager.quit_requested.connect (() => {
                this.handle_real_quit ();
            });

            var quit_action = new GLib.SimpleAction ("quit", null);
            quit_action.activate.connect (() => {
                this.handle_real_quit ();
            });
            this.add_action (quit_action);

            var about_action = new GLib.SimpleAction ("about", null);
            about_action.activate.connect (() => {
                this.show_about ();
            });
            this.add_action (about_action);

            this.set_accels_for_action ("app.quit", { "<Control>q" });
            this.set_accels_for_action ("win.new-profile", { "<Control>n" });

            // 注册系统退出信号，确保异常中断时彻底清理防火墙与 cgroup，无系统残余
            GLib.Unix.signal_add (Posix.Signal.INT, () => {
                this.handle_real_quit ();
                return GLib.Source.REMOVE;
            });
            GLib.Unix.signal_add (Posix.Signal.TERM, () => {
                this.handle_real_quit ();
                return GLib.Source.REMOVE;
            });
        }

        public void handle_real_quit () {
            this.cleanup_runtime ();
            this.quit ();
        }

        public override void shutdown () {
            this.cleanup_runtime ();
            base.shutdown ();
        }

        private void cleanup_runtime () {
            if (this.cleanup_completed) {
                return;
            }
            this.cleanup_completed = true;
            if (this.tunnel_manager != null) {
                this.tunnel_manager.disconnect_tunnel ();
                this.tunnel_manager.cleanup_proxy_runtime ();
            }
        }

        public override void activate () {
            base.activate ();

            if (this.window == null) {
                this.window = new MainWindow (this, this.tunnel_manager);
            }
            this.window.show_and_present ();
        }

        private void show_about () {
            var about = new Adw.AboutDialog ();
            about.application_name = "SShuttle";
            about.application_icon = "network-vpn-symbolic";
            about.developer_name = "1di0t";
            about.version = Config.VERSION;
            about.copyright = "© 2026 1di0t";
            about.present (this.window);
        }
    }
}

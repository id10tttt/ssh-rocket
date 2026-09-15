namespace Sshuttle {

    public class RulesView : Gtk.Box {
        private Adw.ViewStack sub_stack;

        public RulesView (ConfigManager config_manager, TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);

            var section_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 12);
            section_bar.margin_start = 18;
            section_bar.margin_end = 18;
            section_bar.margin_top = 12;
            section_bar.margin_bottom = 12;

            this.sub_stack = new Adw.ViewStack ();
            this.sub_stack.vexpand = true;

            var switcher = new Adw.ViewSwitcher ();
            switcher.stack = this.sub_stack;
            switcher.policy = Adw.ViewSwitcherPolicy.WIDE;
            switcher.hexpand = true;
            section_bar.append (switcher);
            this.append (section_bar);
            this.append (new Gtk.Separator (Gtk.Orientation.HORIZONTAL));
            this.append (this.sub_stack);

            var apps_page = this.create_preferences_page (
                new AppRulesView (config_manager, tunnel_manager)
            );
            var apps_stack_page = this.sub_stack.add_named (apps_page, "apps");
            apps_stack_page.title = "Applications";
            apps_stack_page.icon_name = "application-x-executable-symbolic";

            var domains_page = this.create_preferences_page (
                new DomainRulesView (config_manager, tunnel_manager)
            );
            var domains_stack_page = this.sub_stack.add_named (domains_page, "domains");
            domains_stack_page.title = "Domains & IPs";
            domains_stack_page.icon_name = "network-server-symbolic";

            var blocked_page = this.create_preferences_page (
                new BlacklistRulesView (config_manager, tunnel_manager)
            );
            var blocked_stack_page = this.sub_stack.add_named (blocked_page, "blocked");
            blocked_stack_page.title = "Blocked";
            blocked_stack_page.icon_name = "network-offline-symbolic";
        }

        private Gtk.Widget create_preferences_page (Adw.PreferencesGroup group) {
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;

            var clamp = new Adw.Clamp ();
            clamp.maximum_size = 820;
            clamp.tightening_threshold = 620;
            scrolled.set_child (clamp);

            var page = new Adw.PreferencesPage ();
            page.add (group);
            clamp.set_child (page);
            return scrolled;
        }
    }
}

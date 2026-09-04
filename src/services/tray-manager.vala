namespace Sshuttle {

    [DBus (name = "org.kde.StatusNotifierWatcher")]
    public interface StatusNotifierWatcher : Object {
        public abstract void register_status_notifier_item (string service) throws GLib.Error;
    }

    [DBus (name = "org.kde.StatusNotifierItem")]
    public class StatusNotifierItemService : Object {
        private TrayManager manager;

        public string category { owned get { return "ApplicationStatus"; } }
        public string id { owned get { return "io.github.idi0t.SshuttleGUI"; } }
        public string title { owned get { return "SShuttle"; } }
        public string status { owned get { return "Active"; } }
        public string icon_name {
            owned get {
                var state = this.manager.tunnel_manager.state;
                if (state == TunnelState.CONNECTED) {
                    return "network-vpn-symbolic";
                } else if (state == TunnelState.CONNECTING || state == TunnelState.DISCONNECTING) {
                    return "network-vpn-acquiring-symbolic";
                }
                return "network-vpn-symbolic";
            }
        }
        public ObjectPath menu { owned get { return new ObjectPath ("/MenuBar"); } }
        public bool item_is_menu { get { return true; } }

        public signal void new_icon ();
        public signal void new_status (string status);
        public signal void new_title ();

        public StatusNotifierItemService (TrayManager manager) {
            this.manager = manager;
        }

        public void activate (int x, int y) throws GLib.Error {
            this.manager.show_window_requested ();
        }

        public void context_menu (int x, int y) throws GLib.Error {
            // 右键菜单由 DBusMenu 自动接管
        }

        public void scroll (int delta, string orientation) throws GLib.Error {
        }

        public void emit_icon_changed () {
            this.new_icon ();
        }
    }

    [DBus (name = "com.canonical.dbusmenu")]
    public class DBusMenuService : Object {
        private TrayManager manager;
        private uint revision = 1;

        public uint version { get { return 3; } }
        public string status { owned get { return "normal"; } }

        public signal void layout_updated (uint revision, int parent);

        public DBusMenuService (TrayManager manager) {
            this.manager = manager;
        }

        public void emit_changed () {
            this.revision++;
            this.layout_updated (this.revision, 0);
        }

        public bool get_layout (int parent_id, int recursion_depth, string[] property_names, out uint out_revision, out Variant layout) throws GLib.Error {
            out_revision = this.revision;

            var builder = new VariantBuilder (new VariantType ("(ia{sv}av)"));
            builder.add ("i", 0); // Root ID

            builder.open (new VariantType ("a{sv}"));
            builder.add ("{sv}", "children-display", new Variant.string ("submenu"));
            builder.close ();

            builder.open (new VariantType ("av"));

            // 1. 动态生成服务器列表项
            var profiles = this.manager.config_manager.get_profiles ();
            var active_p = this.manager.config_manager.get_active_profile ();
            string? active_id = (active_p != null) ? active_p.id : null;

            for (int i = 0; i < profiles.length; i++) {
                var p = profiles[i];
                bool is_active = (active_id != null && p.id == active_id);
                string prefix = is_active ? "● " : "○ ";
                string label = prefix + p.name;

                builder.open (new VariantType ("v"));
                this.build_menu_item (builder, 1000 + i, label, true, false);
                builder.close ();
            }

            if (profiles.length > 0) {
                builder.open (new VariantType ("v"));
                this.build_separator (builder, 200);
                builder.close ();
            }

            // 2. Connect / Disconnect 动态控制项
            var state = this.manager.tunnel_manager.state;
            string toggle_label = "Connect";
            if (state == TunnelState.CONNECTED) {
                toggle_label = "Disconnect";
            } else if (state == TunnelState.CONNECTING) {
                toggle_label = "Connecting...";
            } else if (state == TunnelState.DISCONNECTING) {
                toggle_label = "Disconnecting...";
            }

            builder.open (new VariantType ("v"));
            this.build_menu_item (builder, 201, toggle_label, profiles.length > 0, false);
            builder.close ();

            builder.open (new VariantType ("v"));
            this.build_separator (builder, 202);
            builder.close ();

            // 3. Show Window
            builder.open (new VariantType ("v"));
            this.build_menu_item (builder, 203, "Show SShuttle", true, false);
            builder.close ();

            // 4. Quit
            builder.open (new VariantType ("v"));
            this.build_menu_item (builder, 204, "Quit", true, false);
            builder.close ();

            builder.close (); // 结束 av

            layout = builder.end ();
            return true;
        }

        private void build_menu_item (VariantBuilder builder, int id, string label, bool enabled, bool is_separator) {
            builder.open (new VariantType ("(ia{sv}av)"));
            builder.add ("i", id);

            builder.open (new VariantType ("a{sv}"));
            builder.add ("{sv}", "label", new Variant.string (label));
            builder.add ("{sv}", "enabled", new Variant.boolean (enabled));
            builder.add ("{sv}", "visible", new Variant.boolean (true));
            if (is_separator) {
                builder.add ("{sv}", "type", new Variant.string ("separator"));
            }
            builder.close ();

            builder.open (new VariantType ("av"));
            builder.close ();

            builder.close ();
        }

        private void build_separator (VariantBuilder builder, int id) {
            builder.open (new VariantType ("(ia{sv}av)"));
            builder.add ("i", id);

            builder.open (new VariantType ("a{sv}"));
            builder.add ("{sv}", "type", new Variant.string ("separator"));
            builder.add ("{sv}", "visible", new Variant.boolean (true));
            builder.close ();

            builder.open (new VariantType ("av"));
            builder.close ();

            builder.close ();
        }

        public void @event (int id, string event_id, Variant data, uint timestamp) throws GLib.Error {
            if (event_id != "clicked") {
                return;
            }

            if (id >= 1000) {
                int index = id - 1000;
                var profiles = this.manager.config_manager.get_profiles ();
                if (index >= 0 && index < profiles.length) {
                    var target = profiles[index];
                    this.manager.tunnel_manager.set_active_profile (target.id);
                }
            } else if (id == 201) {
                this.manager.tunnel_manager.toggle_connection ();
            } else if (id == 203) {
                this.manager.show_window_requested ();
            } else if (id == 204) {
                this.manager.quit_requested ();
            }
        }
    }

    public class TrayManager : Object {
        public signal void show_window_requested ();
        public signal void quit_requested ();

        public ConfigManager config_manager { get; private set; }
        public TunnelManager tunnel_manager { get; private set; }

        private StatusNotifierItemService sni_service;
        private DBusMenuService menu_service;
        private GLib.DBusConnection? connection = null;

        public TrayManager (ConfigManager config_manager, TunnelManager tunnel_manager) {
            this.config_manager = config_manager;
            this.tunnel_manager = tunnel_manager;

            this.sni_service = new StatusNotifierItemService (this);
            this.menu_service = new DBusMenuService (this);

            this.tunnel_manager.state_changed.connect (() => {
                this.sni_service.emit_icon_changed ();
                this.menu_service.emit_changed ();
            });

            this.tunnel_manager.profile_changed.connect (() => {
                this.menu_service.emit_changed ();
            });

            this.init_dbus.begin ();
        }

        private async void init_dbus () {
            try {
                this.connection = yield GLib.Bus.get (GLib.BusType.SESSION, null);

                // 注册 SNI 服务与 DBusMenu
                this.connection.register_object ("/StatusNotifierItem", this.sni_service);
                this.connection.register_object ("/MenuBar", this.menu_service);

                // 向 StatusNotifierWatcher 注册
                var watcher = yield this.connection.get_proxy<StatusNotifierWatcher> (
                    "org.kde.StatusNotifierWatcher",
                    "/StatusNotifierWatcher"
                );
                watcher.register_status_notifier_item ("/StatusNotifierItem");

            } catch (GLib.Error e) {
                // 若桌面环境未运行 StatusNotifierWatcher，优雅降级，不阻断程序运行
            }
        }
    }
}

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
                return "network-vpn-disconnected-symbolic";
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
        }

        public void scroll (int delta, string orientation) throws GLib.Error {
        }

        [DBus (visible = false)]
        public void emit_icon_changed () {
            this.new_icon ();
        }
    }

    /**
     * 匹配 com.canonical.dbusmenu 规范的 D-Bus 服务，
     * 使用结构体确保 Vala 生成正确的 (ia{sv}av) 与 a(ia{sv}) 签名。
     */
    public struct MenuItemLayout {
        public int id;
        public GLib.HashTable<string, Variant> properties;
        public Variant[] children;
    }

    public struct MenuProperties {
        public int id;
        public GLib.HashTable<string, Variant> properties;
    }

    [DBus (name = "com.canonical.dbusmenu")]
    public class DBusMenuService : Object {
        private TrayManager manager;
        private uint revision = 1;

        public uint version { get { return 3; } }
        [DBus (name = "TextDirection")]
        public string text_direction { owned get { return "ltr"; } }
        [DBus (name = "Status")]
        public string status { owned get { return "normal"; } }
        [DBus (name = "IconThemePath")]
        public string[] icon_theme_path { owned get { return new string[0]; } }

        public signal void layout_updated (uint revision, int parent);

        public DBusMenuService (TrayManager manager) {
            this.manager = manager;
        }

        [DBus (visible = false)]
        public void emit_changed () {
            this.revision++;
            this.layout_updated (this.revision, 0);
        }

        // 构建单个菜单项的 Variant
        private Variant make_item_variant (int id, string label, bool enabled) {
            var b = new VariantBuilder (new VariantType ("(ia{sv}av)"));
            b.add ("i", id);

            b.open (new VariantType ("a{sv}"));
            b.add ("{sv}", "label", new Variant.string (label));
            b.add ("{sv}", "enabled", new Variant.boolean (enabled));
            b.add ("{sv}", "visible", new Variant.boolean (true));
            b.close ();

            b.open (new VariantType ("av"));
            b.close ();

            return b.end ();
        }

        // 构建分隔符
        private Variant make_separator_variant (int id) {
            var b = new VariantBuilder (new VariantType ("(ia{sv}av)"));
            b.add ("i", id);

            b.open (new VariantType ("a{sv}"));
            b.add ("{sv}", "type", new Variant.string ("separator"));
            b.add ("{sv}", "visible", new Variant.boolean (true));
            b.close ();

            b.open (new VariantType ("av"));
            b.close ();

            return b.end ();
        }

        /**
         * 返回菜单树布局，签名严格匹配规范 (i, i, as) -> (u, (ia{sv}av))
         */
        public void get_layout (int parent_id, int recursion_depth, string[] property_names, out uint out_revision, out MenuItemLayout layout) throws GLib.Error {
            out_revision = this.revision;

            // 动态生成子项列表
            var items = new GLib.GenericArray<Variant> ();

            // 1. 服务器列表
            var profiles = this.manager.config_manager.get_profiles ();
            var active_p = this.manager.config_manager.get_active_profile ();
            string? active_id = (active_p != null) ? active_p.id : null;

            for (int i = 0; i < profiles.length; i++) {
                var p = profiles[i];
                bool is_active = (active_id != null && p.id == active_id);
                string prefix = is_active ? "● " : "○ ";
                items.add (this.make_item_variant (1000 + i, prefix + p.name, true));
            }

            if (profiles.length > 0) {
                items.add (this.make_separator_variant (200));
            }

            // 2. Connect / Disconnect
            var state = this.manager.tunnel_manager.state;
            string toggle_label = "Connect";
            if (state == TunnelState.CONNECTED) {
                toggle_label = "Disconnect";
            } else if (state == TunnelState.CONNECTING) {
                toggle_label = "Connecting…";
            } else if (state == TunnelState.DISCONNECTING) {
                toggle_label = "Disconnecting…";
            }
            items.add (this.make_item_variant (201, toggle_label, profiles.length > 0));

            items.add (this.make_separator_variant (202));

            // 3. Show Window
            items.add (this.make_item_variant (203, "Show SShuttle", true));

            // 4. Quit
            items.add (this.make_item_variant (204, "Quit", true));

            // 组装根节点
            var root_props = new GLib.HashTable<string, Variant> (GLib.str_hash, GLib.str_equal);
            root_props.insert ("children-display", new Variant.string ("submenu"));

            var children = new Variant[items.length];
            for (int i = 0; i < items.length; i++) {
                children[i] = items[i];
            }

            layout = MenuItemLayout ();
            layout.id = 0;
            layout.properties = root_props;
            layout.children = children;
        }

        /**
         * 返回指定 ID 的属性，签名 (ai, as) -> a(ia{sv})
         */
        public void get_group_properties (int[] ids, string[] property_names, out MenuProperties[] properties) throws GLib.Error {
            properties = new MenuProperties[0];
        }

        /**
         * 菜单即将显示回调，签名 (i) -> (b)
         */
        public bool about_to_show (int id) throws GLib.Error {
            return false;
        }

        /**
         * 菜单项点击事件，签名 (i, s, v, u) -> ()
         */
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

                this.connection.register_object ("/StatusNotifierItem", this.sni_service);
                this.connection.register_object ("/MenuBar", this.menu_service);

                var watcher = yield this.connection.get_proxy<StatusNotifierWatcher> (
                    "org.kde.StatusNotifierWatcher",
                    "/StatusNotifierWatcher"
                );
                watcher.register_status_notifier_item ("/StatusNotifierItem");

            } catch (GLib.Error e) {
                // 若桌面环境未运行 StatusNotifierWatcher，优雅降级
            }
        }
    }
}

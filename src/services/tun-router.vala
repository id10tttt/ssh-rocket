namespace Sshuttle {
    /** 只管理本会话创建的虚拟网卡和打标策略路由，不修改默认路由。 */
    public class TunRouter : Object {
        public const string DEVICE = "sshrocket0";
        public const string MARK = "0x5352";
        public const string TABLE = "21330";
        private bool device_created;
        private bool rule_v4;
        private bool rule_v6;

        private string run (string[] args) throws GLib.Error {
            string[] command = { Config.IP_PATH };
            foreach (var arg in args) command += arg;
            var launcher = new GLib.SubprocessLauncher (GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE);
            var child = Native.spawnv (launcher, command);
            string output, errors;
            child.communicate_utf8 (null, null, out output, out errors);
            if (!child.get_successful ()) {
                throw new GLib.IOError.FAILED ("ip %s: %s", string.joinv (" ", args), errors.strip ());
            }
            return output;
        }

        public void start (uint uid, bool ipv6) throws GLib.Error {
            // 不复用或覆盖已有接口、路由表及规则；失败仅撤销本次已创建的对象。
            if (GLib.FileUtils.test ("/sys/class/net/" + DEVICE, GLib.FileTest.EXISTS)) {
                throw new GLib.IOError.BUSY ("SSH Rocket TUN interface already exists");
            }
            foreach (var family in new string[] { "-4", "-6" }) {
                var rules = run ({ family, "rule", "show" });
                if (TABLE in rules || MARK in rules ||
                    run ({ family, "route", "show", "table", "all" }).contains ("table " + TABLE)) {
                    throw new GLib.IOError.BUSY ("SSH Rocket routing table or mark is already in use");
                }
            }
            try {
                run ({ "tuntap", "add", "dev", DEVICE, "mode", "tun", "user", uid.to_string () });
                device_created = true;
                run ({ "address", "add", "198.18.0.1/32", "dev", DEVICE });
                run ({ "link", "set", "dev", DEVICE, "mtu", "1500", "up" });
                run ({ "-4", "route", "add", "default", "dev", DEVICE, "table", TABLE });
                run ({ "-4", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE });
                rule_v4 = true;
                if (ipv6) {
                    run ({ "-6", "address", "add", "fd00:5352::1/128", "dev", DEVICE, "nodad" });
                    run ({ "-6", "route", "add", "default", "dev", DEVICE, "table", TABLE });
                    run ({ "-6", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE });
                    rule_v6 = true;
                }
            } catch (GLib.Error e) {
                stop ();
                throw e;
            }
        }

        public void stop () {
            try {
                if (rule_v4) {
                    run ({ "-4", "rule", "del", "priority", TABLE, "fwmark", MARK, "lookup", TABLE });
                    rule_v4 = false;
                }
            } catch (GLib.Error e) { warning ("IPv4 cleanup: %s", e.message); }
            try {
                if (rule_v6) {
                    run ({ "-6", "rule", "del", "priority", TABLE, "fwmark", MARK, "lookup", TABLE });
                    rule_v6 = false;
                }
            } catch (GLib.Error e) { warning ("IPv6 cleanup: %s", e.message); }
            try {
                if (device_created) {
                    run ({ "link", "del", "dev", DEVICE });
                    device_created = false;
                }
            } catch (GLib.Error e) { warning ("TUN cleanup: %s", e.message); }
        }
    }
}

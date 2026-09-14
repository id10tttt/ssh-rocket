namespace Sshuttle {
    [DBus (name = "io.github.idi0t.SshuttleGUI.Runtime")]
    public interface Runtime : Object {
        public abstract bool prepare () throws GLib.Error;
        public abstract bool cgroup (string operation, int pid) throws GLib.Error;
        public abstract bool has_active_tables () throws GLib.Error;
        public abstract bool base_chains_exist (int port, bool ipv6) throws GLib.Error;
        public abstract bool apply_routing (int port, bool ipv6, string policy, string[] networks,
            string[] patterns, string[] actions) throws GLib.Error;
        public abstract bool apply_blacklist () throws GLib.Error;
        public abstract void add_ips (string[] ips, string action) throws GLib.Error;
        public abstract void flush_ips () throws GLib.Error;
        public abstract void cleanup () throws GLib.Error;
        public abstract void start_tunnel (string[] argv, string password, string agent) throws GLib.Error;
        public abstract void stop_tunnel () throws GLib.Error;
        public signal void log_line (string line);
        public signal void tunnel_exited (int status, int signal_number);
    }

    /** 在当前 GUI 会话内复用一次授权的特权连接。 */
    public class RuntimeClient : Object {
        public static Runtime? proxy = null;
        private static GLib.Subprocess? helper;
        private static GLib.DBusConnection? connection;
        private static bool starting = false;
        private static GLib.Cancellable? startup_cancel;
        public signal void lost ();
        private static RuntimeClient? instance;

        public static RuntimeClient get_default () {
            if (instance == null) {
                instance = new RuntimeClient ();
            }
            return instance;
        }

        public static string helper_path () {
            return Config.HELPER_PATH;
        }

        public static async void ensure_started () throws GLib.Error {
            if (proxy != null) {
                return;
            }
            if (starting) {
                throw new GLib.IOError.PENDING ("Administrator authorization is already in progress");
            }
            starting = true;
            startup_cancel = new GLib.Cancellable ();
            try {
                helper = new GLib.Subprocess (GLib.SubprocessFlags.STDIN_PIPE | GLib.SubprocessFlags.STDOUT_PIPE,
                    Config.PKEXEC_PATH, helper_path (), null);
                var output = new GLib.DataInputStream (helper.get_stdout_pipe ());
                string? address = yield output.read_line_async (GLib.Priority.DEFAULT, startup_cancel);
                if (address == null || !address.has_prefix ("unix:")) {
                    throw new GLib.IOError.PERMISSION_DENIED ("Administrator authorization failed or was cancelled");
                }
                connection = yield new GLib.DBusConnection.for_address (address,
                    GLib.DBusConnectionFlags.AUTHENTICATION_CLIENT, null, startup_cancel);
                connection.set_exit_on_close (false);
                proxy = yield connection.get_proxy<Runtime> (null, "/io/github/idi0t/SshuttleGUI/Runtime",
                    GLib.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES, startup_cancel);
                var session_connection = connection;
                connection.on_closed.connect (() => {
                    if (connection == session_connection) {
                        proxy = null;
                        get_default ().lost ();
                    }
                });
            } catch (GLib.Error e) {
                shutdown ();
                throw e;
            } finally {
                starting = false;
                startup_cancel = null;
            }
        }

        public static void shutdown () {
            if (startup_cancel != null) {
                startup_cancel.cancel ();
            }
            proxy = null;
            if (connection != null) {
                connection.close.begin ();
                connection = null;
            }
            if (helper != null) {
                try {
                    helper.get_stdin_pipe ().close ();
                } catch (GLib.Error e) {
                    warning ("Failed to close helper session: %s", e.message);
                }
                helper.wait_async.begin ();
                helper = null;
            }
        }
    }
}

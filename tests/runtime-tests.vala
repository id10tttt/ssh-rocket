string[] tunnel_args (string remote = "exit") {
    return { "sshuttle", "-l", "127.0.0.1:12300", "--method", "nft", "-v",
        "-e", "ssh -i '/tmp/key with spaces'", "-r", remote, "0.0.0.0/0" };
}

void test_prepare () {
    var service = new Sshuttle.RuntimeService ((uint) Posix.getuid ());
    try {
        assert (service.prepare ());
        assert (Sshuttle.CgroupManager.proxy_created);
        assert (Sshuttle.CgroupManager.block_created);
        assert (service.prepare ());
        assert (!service.cgroup ("proxy", 1));
        service.cleanup ();
        assert (!Sshuttle.CgroupManager.proxy_created);
        assert (!Sshuttle.CgroupManager.block_created);
    } catch (GLib.Error e) {
        GLib.error ("Prepare failed: %s", e.message);
    }
}

void test_reject_arguments () {
    var service = new Sshuttle.RuntimeService ((uint) Posix.getuid ());
    try {
        service.prepare ();
        string[] invalid_options = { "--daemon", "--auto-hosts", "--namespace", "--firewall", "--python" };
        foreach (var option in invalid_options) {
            string[] args = tunnel_args ();
            args += option;
            try {
                service.start_tunnel (args, "", "");
                assert_not_reached ();
            } catch (GLib.IOError.INVALID_ARGUMENT e) {}
        }
        string[] invalid_listeners = { "0.0.0.0:12300", "127.0.0.1:22", "127.0.0.1:70000",
            "127.0.0.1:12300,[::1]:12301" };
        foreach (var listener in invalid_listeners) {
            string[] args = tunnel_args ();
            args[2] = listener;
            try {
                service.start_tunnel (args, "", "");
                assert_not_reached ();
            } catch (GLib.IOError.INVALID_ARGUMENT e) {}
        }
        service.cleanup ();
    } catch (GLib.Error e) {
        GLib.error ("Argument test failed: %s", e.message);
    }
}

void test_lifecycle (bool stop) {
    var service = new Sshuttle.RuntimeService ((uint) Posix.getuid ());
    var loop = new GLib.MainLoop ();
    int exits = 0;
    int cleaned_before = Sshuttle.CgroupManager.cleanup_count;
    uint guard = GLib.Timeout.add_seconds (8, () => {
        GLib.Test.message ("Tunnel lifecycle timed out");
        assert_not_reached ();
    });
    service.tunnel_exited.connect ((status, signal_number) => {
        assert (!Sshuttle.CgroupManager.proxy_created);
        assert (!Sshuttle.CgroupManager.block_created);
        if (stop) {
            assert (status == -1);
            assert (signal_number == Posix.Signal.INT);
        } else {
            assert (status == 0);
            assert (signal_number == 0);
        }
        exits++;
        loop.quit ();
    });
    try {
        for (int i = 0; i < 2; i++) {
            assert (service.prepare ());
            service.start_tunnel (tunnel_args (stop ? "wait" : "exit"), "test-password", "test-agent");
            assert (service.apply_blacklist ());
            if (stop) {
                service.cleanup ();
                // 清理必须等子进程退出，不能先删正在引用的 cgroup。
                assert (Sshuttle.CgroupManager.proxy_created);
                assert (Sshuttle.CgroupManager.block_created);
            }
            loop.run ();
        }
        assert (exits == 2);
        assert (Sshuttle.CgroupManager.cleanup_count == cleaned_before + 2);
        service.cleanup ();
        assert (Sshuttle.CgroupManager.cleanup_count == cleaned_before + 2);
    } catch (GLib.Error e) {
        GLib.error ("Lifecycle failed: %s", e.message);
    }
    GLib.Source.remove (guard);
}

void test_cancelled_authorization () {
    var loop = new GLib.MainLoop ();
    Sshuttle.RuntimeClient.ensure_started.begin ((obj, result) => {
        try {
            Sshuttle.RuntimeClient.ensure_started.end (result);
            assert_not_reached ();
        } catch (GLib.Error e) {
            assert (Sshuttle.RuntimeClient.proxy == null);
        }
        loop.quit ();
    });
    loop.run ();
}

int run_ipc_client (string address) {
    try {
        var connection = new GLib.DBusConnection.for_address_sync (address,
            GLib.DBusConnectionFlags.AUTHENTICATION_CLIENT);
        connection.set_exit_on_close (false);
        var proxy = connection.get_proxy_sync<Sshuttle.Runtime> (null,
            "/io/github/idi0t/SshuttleGUI/Runtime", GLib.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES);
        assert (proxy.prepare ());
        var loop = new GLib.MainLoop ();
        proxy.tunnel_exited.connect ((status, signal_number) => {
            assert (status == 0 && signal_number == 0);
            loop.quit ();
        });
        proxy.start_tunnel (tunnel_args (), "test-password", "test-agent");
        loop.run ();
        proxy.cleanup ();
        connection.close_sync ();
        return 0;
    } catch (GLib.Error e) {
        stderr.printf ("IPC client failed: %s\n", e.message);
        return 1;
    }
}

void test_private_connection () {
    var loop = new GLib.MainLoop ();
    var service = new Sshuttle.RuntimeService ((uint) Posix.getuid ());
    GLib.DBusConnection? active_connection = null;
    try {
        var observer = new GLib.DBusAuthObserver ();
        observer.authorize_authenticated_peer.connect ((stream, credentials) => {
            try {
                return credentials != null && credentials.get_unix_user () == Posix.getuid ();
            } catch (GLib.Error e) { return false; }
        });
        var server = new GLib.DBusServer.sync ("unix:abstract=sshuttle-gui-test-" + GLib.Uuid.string_random (), GLib.DBusServerFlags.NONE,
            GLib.DBus.generate_guid (), observer);
        server.new_connection.connect ((connection) => {
            try {
                connection.register_object<Sshuttle.Runtime> ("/io/github/idi0t/SshuttleGUI/Runtime", service);
                active_connection = connection;
                return true;
            } catch (GLib.Error e) { return false; }
        });
        server.start ();
        var client = new GLib.Subprocess (GLib.SubprocessFlags.NONE,
            "/proc/self/exe", "--ipc-client", server.get_client_address (), null);
        client.wait_async.begin (null, (obj, result) => {
            try {
                client.wait_async.end (result);
                assert (client.get_successful ());
            } catch (GLib.Error e) { assert_not_reached (); }
            loop.quit ();
        });
        loop.run ();
        server.stop ();
        assert (active_connection != null);
    } catch (GLib.Error e) {
        GLib.error ("Private connection test failed: %s", e.message);
    }
}

int runtime_test_main (string[] args) {
    if (args.length == 3 && args[1] == "--ipc-client") {
        return run_ipc_client (args[2]);
    }
    // 子进程模拟 sshuttle，仅校验传参和环境，不申请任何权限。
    if (args.length > 1 && args[1] == "-l") {
        assert (GLib.Environment.get_variable ("SSHPASS") == "test-password");
        assert (GLib.Environment.get_variable ("SSH_AUTH_SOCK") == "test-agent");
        foreach (var arg in args) {
            assert (!("test-password" in arg));
            if (arg == "wait") {
                GLib.Thread.usleep (10000000);
            }
        }
        return 0;
    }
    GLib.Test.init (ref args);
    GLib.Test.add_func ("/runtime/prepare-empty-targets", test_prepare);
    GLib.Test.add_func ("/runtime/reject-arguments", test_reject_arguments);
    GLib.Test.add_func ("/runtime/exit-and-reconnect", () => { test_lifecycle (false); });
    GLib.Test.add_func ("/runtime/signal-and-reconnect", () => { test_lifecycle (true); });
    GLib.Test.add_func ("/runtime/authorization-failure", test_cancelled_authorization);
    GLib.Test.add_func ("/runtime/private-connection", test_private_connection);
    return GLib.Test.run ();
}

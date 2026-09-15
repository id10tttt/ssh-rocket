// 在独立网络命名空间内运行；仅用现有 cgroup 替代不可创建的宿主根 cgroup。
namespace Config {
    public const string NFT_PATH = "/proc/self/exe";
    public const string IP_PATH = "/usr/sbin/ip";
    public const string PKEXEC_PATH = "/usr/bin/false";
    public const string HELPER_PATH = "/usr/bin/false";
}

string command_output (string[] args) throws GLib.Error {
    var launcher = new GLib.SubprocessLauncher (GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE);
    var child = Sshuttle.Native.spawnv (launcher, args);
    string output, errors;
    child.communicate_utf8 (null, null, out output, out errors);
    if (!child.get_successful ()) throw new GLib.IOError.FAILED ("%s: %s", args[0], errors);
    return output;
}

int nft_adapter (string[] args) {
    string proxy_group = GLib.Environment.get_variable ("ROCKET_TEST_PROXY_GROUP") ?? "user.slice";
    string[] command = { "/usr/sbin/nft" };
    for (int i = 1; i < args.length; i++) {
        command += args[i].replace ("sshrocket-runtime", "init.scope")
            .replace ("sshuttle-proxy", proxy_group).replace ("sshuttle-block", "system.slice");
    }
    try {
        stdout.printf ("%s", command_output (command).replace ("\"" + proxy_group + "\"", "\"sshuttle-proxy\""));
        return 0;
    } catch (GLib.Error e) { stderr.printf ("%s\n", e.message); return 1; }
}

string trace_ip () throws GLib.Error {
    string output = command_output ({ "/usr/bin/curl", "--noproxy", "*", "--silent", "--show-error",
        "--max-time", "15", "https://1.1.1.1/cdn-cgi/trace" });
    foreach (var line in output.split ("\n")) if (line.has_prefix ("ip=")) return line.substring (3);
    throw new GLib.IOError.FAILED ("Trace response has no IP");
}

int main (string[] args) {
    if (args.length < 2 || args[1] != "--run") return nft_adapter (args);
    if (Posix.geteuid () != 0 || args.length != 4) return 2;
    var router = new Sshuttle.TunRouter ();
    var nft = new Sshuttle.NftManager ();
    GLib.Subprocess? ssh = null;
    GLib.Subprocess? tun = null;
    int result = 1;
    try {
        string baseline = trace_ip ();
        string[] excludes = { args[2], "127.0.0.0/8", "::1/128" };
        var profile = new Sshuttle.Profile ();
        profile.host = args[2];
        profile.username = "debian";
        profile.ipv6 = true;
        var built = Sshuttle.CommandBuilder.build_argv (profile, 12300);
        string ssh_command = "";
        for (int i = 0; i < built.length - 1; i++) if (built[i] == "-e") ssh_command = built[i + 1];
        string home = GLib.Environment.get_variable ("ROCKET_TEST_HOME");
        // 命名空间 UID 映射使系统 ssh_config 的所有者检查失效；其余参数使用生产构造器。
        ssh_command = ssh_command.replace ("ssh ", "ssh -F /dev/null -o UserKnownHostsFile=" +
            GLib.Shell.quote (home + "/.ssh/known_hosts") + " -i " + GLib.Shell.quote (home + "/.ssh/id_ed25519") +
            " -i " + GLib.Shell.quote (home + "/.ssh/id_rsa") + " ");
        string[] ssh_args;
        GLib.Shell.parse_argv (ssh_command + " -- " + GLib.Shell.quote (profile.host), out ssh_args);
        var launcher = new GLib.SubprocessLauncher (GLib.SubprocessFlags.NONE);
        ssh = Sshuttle.Native.spawnv (launcher, ssh_args);
        string proxy_ip = "";
        for (int i = 0; i < 20; i++) {
            try {
                var output = command_output ({ "/usr/bin/curl", "--noproxy", "", "--socks5-hostname", "127.0.0.1:12300",
                    "--silent", "--show-error", "--max-time", "3", "https://1.1.1.1/cdn-cgi/trace" });
                foreach (var line in output.split ("\n")) if (line.has_prefix ("ip=")) proxy_ip = line.substring (3);
                if (proxy_ip != "") break;
            } catch (GLib.Error e) {}
            GLib.Thread.usleep (100000);
        }
        assert (proxy_ip != "");
        stdout.printf ("PASS OpenSSH SOCKS: direct=%s proxy=%s\n", baseline, proxy_ip);
        uint8[] query = { 0x53, 0x52, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 3, 'c', 'o', 'm', 0, 0, 1, 0, 1 };
        var response = Sshuttle.DnsProxy.query_tcp (12301, query);
        assert (response != null && Sshuttle.DnsProxy.parse_answer_ips (response).length > 0);
        stdout.printf ("PASS DNS over OpenSSH TCP forwarding\n");
        router.start (0, true);
        tun = Sshuttle.Native.spawnv (launcher, { args[3], "--device", "tun://sshrocket0",
            "--proxy", "socks5://127.0.0.1:12300", "--loglevel", "info" });
        GLib.Thread.usleep (300000);
        assert (nft.create_base_chains (12300, true, { "0.0.0.0/0", "::/0" }));
        assert (nft.base_chains_exist (12300, true));
        assert (nft.apply_cgroup_filter (12300, true, "proxy", excludes, {}));
        assert (trace_ip () == proxy_ip);
        string marked_route = command_output ({ "/usr/sbin/ip", "-4", "route", "get", "1.1.1.1", "mark", "0x5352" });
        assert ("sshrocket0" in marked_route);
        stdout.printf ("PASS transparent TCP via production nftables + TUN + tun2socks\n");
        assert (nft.apply_cgroup_filter (12300, true, "direct", excludes, {}));
        assert (trace_ip () == proxy_ip);
        stdout.printf ("PASS selected application overrides default direct\n");
        nft.add_ips_to_set ({ "1.1.1.1" }, "direct");
        assert (trace_ip () == baseline);
        stdout.printf ("PASS direct domain/IP exception overrides selected application\n");
        nft.flush_ip_sets ();
        assert (trace_ip () == proxy_ip);
        GLib.Environment.set_variable ("ROCKET_TEST_PROXY_GROUP", "system.slice", true);
        // 先以原映射删除旧 cgroup 规则，随后模拟未选中应用。
        GLib.Environment.set_variable ("ROCKET_TEST_PROXY_GROUP", "user.slice", true);
        nft.remove_cgroup_filter (12300, true);
        GLib.Environment.set_variable ("ROCKET_TEST_PROXY_GROUP", "system.slice", true);
        assert (nft.apply_cgroup_filter (12300, true, "direct", excludes, {}));
        assert (trace_ip () == baseline);
        nft.add_ips_to_set ({ "1.1.1.1" }, "proxy");
        assert (trace_ip () == proxy_ip);
        stdout.printf ("PASS unselected application direct; explicit proxy IP overrides default\n");
        string ipv6_route = command_output ({ "/usr/sbin/ip", "-6", "route", "get", "2606:4700:4700::1111", "mark", "0x5352" });
        assert ("sshrocket0" in ipv6_route);
        stdout.printf ("PASS IPv6 policy route installation\n");
        result = 0;
    } catch (GLib.Error e) { stderr.printf ("FAIL %s\n", e.message); }
    nft.cleanup_all_sshuttle_tables (12300);
    if (tun != null) { tun.force_exit (); try { tun.wait (); } catch (GLib.Error e) {} }
    if (ssh != null) { ssh.force_exit (); try { ssh.wait (); } catch (GLib.Error e) {} }
    router.stop ();
    try {
        assert (!("sshrocket" in command_output ({ "/usr/sbin/nft", "list", "tables" })));
        assert (!("21330" in command_output ({ "/usr/sbin/ip", "rule", "show" })));
        assert (!("21330" in command_output ({ "/usr/sbin/ip", "-6", "rule", "show" })));
        assert (!GLib.FileUtils.test ("/sys/class/net/sshrocket0", GLib.FileTest.EXISTS));
        stdout.printf ("PASS cleanup: no owned TUN, nftables tables or policy rules remain\n");
    } catch (GLib.Error e) { return 1; }
    return result;
}

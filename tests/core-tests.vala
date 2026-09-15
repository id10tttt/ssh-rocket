void test_commands () {
    var profile = new Sshuttle.Profile ();
    profile.host = "test-host";
    profile.username = "debian";
    profile.port = 2200;
    profile.auth_type = "key";
    profile.key_path = "/tmp/key with spaces";
    var settings = new Sshuttle.NetworkSettings ();
    settings.ipv6 = true;
    try {
        var args = Sshuttle.CommandBuilder.build_argv (profile, settings, 12300);
        assert (args[0] == "ssh-rocket");
        string command = "";
        for (int i = 1; i < args.length; i++) if (args[i] == "-e") command = args[i + 1];
        string[] ssh_args;
        GLib.Shell.parse_argv (command, out ssh_args);
        assert ("-N" in ssh_args && "-T" in ssh_args);
        assert ("127.0.0.1:12300" in ssh_args);
        assert ("127.0.0.1:12301:1.1.1.1:53" in ssh_args);
        assert ("/tmp/key with spaces" in ssh_args);
        assert ("2200" in ssh_args && "debian" in ssh_args);
        assert ("::/0" in args && "0.0.0.0/0" in args);
        profile.auth_type = "password";
        profile.password = "secret-must-not-appear";
        settings.dns = false;
        args = Sshuttle.CommandBuilder.build_argv (profile, settings);
        var joined = string.joinv (" ", args);
        assert (!(profile.password in joined));
        assert (!("1.1.1.1:53" in joined));
        assert ("sshpass -e ssh" in joined);
        profile.host = "-oProxyCommand=bad";
        try {
            Sshuttle.CommandBuilder.build_argv (profile, settings);
            assert_not_reached ();
        } catch (GLib.IOError.INVALID_ARGUMENT e) {}
    } catch (GLib.Error e) { GLib.error ("Command test: %s", e.message); }
}

void test_rule_compatibility () {
    var config = new Sshuttle.ConfigManager ();
    var profile = new Sshuttle.Profile ();
    profile.host = "test-host";
    var settings = config.get_network_settings ();
    settings.exclude = { "10.0.0.0/8", "10.0.0.0/8" };
    config.set_network_settings (settings);
    config.save_profile (profile);
    config.set_active_profile (profile.id);
    config.set_domain_rules ({ new Sshuttle.DomainRule ("direct.example.com", "direct"),
        new Sshuttle.DomainRule ("*.example.com", "proxy") }, "direct");
    var resolver = new Sshuttle.DnsProxy (config, new Sshuttle.NftManager ());
    bool matched;
    assert (resolver.resolve_action_for_domain ("www.example.com", out matched) == "proxy" && matched);
    assert (resolver.resolve_action_for_domain ("direct.example.com", out matched) == "direct" && matched);
    assert (resolver.resolve_action_for_domain ("unmatched.test", out matched) == "direct" && !matched);
    var restored = Sshuttle.Profile.deserialize (profile.serialize ().get_object ());
    assert (restored.id == profile.id && restored.host == profile.host);
    var profile_json = profile.serialize ().get_object ();
    assert (!profile_json.has_member ("routes") && !profile_json.has_member ("dns"));
    var restored_settings = new Sshuttle.ConfigManager ().get_network_settings ();
    assert (restored_settings.exclude.length == 2 && restored_settings.exclude[0] == "10.0.0.0/8");
    int count = 0;
    foreach (var value in Sshuttle.CommandBuilder.get_effective_excludes (profile, settings))
        if (value == "10.0.0.0/8") count++;
    assert (count == 1);
}

void test_shadowrocket_rules () {
    string source = """
[General]
skip-proxy = 10.0.0.0/8, *.lan
[Rule]
DOMAIN-SUFFIX,ads.example,Reject
DOMAIN,api.example,Direct
DOMAIN-KEYWORD,blocked,Proxy
IP-CIDR,203.0.113.0/24,Reject
RULE-SET,https://example.com/nested.list,Proxy
FINAL,direct
[URL Rewrite]
^https://example.com https://example.org 302
""";
    var imported = Sshuttle.RuleImporter.import_from_string (source);
    assert (imported.default_policy == "direct");
    assert (imported.direct_count == 3);
    assert (imported.proxy_count == 1);
    assert (imported.reject_count == 2);
    assert (imported.rule_sets.length == 1);
    assert (imported.ignored_count == 1);

    var matcher = new Sshuttle.DomainRuleMatcher ({
        new Sshuttle.DomainRule ("safe.ads.example", "direct"),
        imported.rules[2],
        imported.rules[3],
        imported.rules[4]
    }, imported.default_policy);
    bool matched;
    assert (matcher.resolve ("safe.ads.example", out matched) == "direct" && matched);
    assert (matcher.resolve ("www.ads.example", out matched) == "reject" && matched);
    assert (matcher.resolve ("blocked-site.test", out matched) == "proxy" && matched);
    assert (matcher.resolve ("unmatched.test", out matched) == "direct" && !matched);

    try {
        var config = new Sshuttle.ConfigManager ();
        config.set_imported_rule_source (imported, "https://example.com/rules.conf", "Test Rules");
        config.add_domain_rule ("safe.ads.example", "direct");
        assert (config.resolve_domain_action ("safe.ads.example", out matched) == "direct" && matched);
        assert (config.resolve_domain_action ("www.ads.example", out matched) == "reject" && matched);
        assert (config.get_network_rules ().length == 2);

        var restored_config = new Sshuttle.ConfigManager ();
        assert (restored_config.get_imported_rule_count () == imported.rules.length);
        assert (restored_config.resolve_domain_action ("www.ads.example", out matched) == "reject" && matched);
    } catch (GLib.Error e) {
        GLib.error ("Rule cache test: %s", e.message);
    }

    uint8[] query = {
        0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01, 'a', 0x04, 't', 'e', 's', 't', 0x00, 0x00, 0x01, 0x00, 0x01
    };
    var response = Sshuttle.DnsProxy.build_error_response (query, 3);
    assert (response.length == query.length && (response[3] & 0x0f) == 3);
}

void test_dns_tcp () {
    try {
        var listener = new GLib.SocketListener ();
        uint16 port = listener.add_any_inet_port (null);
        uint8[] packet = { 0x12, 0x34, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0 };
        var thread = new GLib.Thread<void*> ("dns-fixture", () => {
            try {
                var peer = listener.accept ();
                uint8[] request = new uint8[14];
                size_t count;
                peer.input_stream.read_all (request, out count);
                assert (count == 14 && request[0] == 0 && request[1] == 12);
                request[4] |= 0x80;
                // 分段写入，验证读取端不会把一次 read 当成完整 DNS 帧。
                foreach (var value in request) {
                    peer.output_stream.write_all ({ value }, out count);
                    GLib.Thread.usleep (1000);
                }
                peer.close ();
            } catch (GLib.Error e) { GLib.error ("DNS fixture: %s", e.message); }
            return null;
        });
        var response = Sshuttle.DnsProxy.query_tcp (port, packet);
        assert (response != null && response.length == 12 && response[0] == 0x12 && response[2] == 0x81);
        thread.join ();
        listener.close ();
        assert (Sshuttle.DnsProxy.query_tcp (port, packet) == null);
    } catch (GLib.Error e) { GLib.error ("DNS test: %s", e.message); }
}

int main (string[] args) {
    GLib.Test.init (ref args);
    try {
        GLib.Environment.set_variable ("SSHUTTLE_CONFIG_DIR", GLib.DirUtils.make_tmp ("ssh-rocket-tests-XXXXXX"), true);
    } catch (GLib.Error e) { return 1; }
    GLib.Test.add_func ("/ssh/command-auth-and-routes", test_commands);
    GLib.Test.add_func ("/ssh/rules-and-profile-compatibility", test_rule_compatibility);
    GLib.Test.add_func ("/ssh/shadowrocket-rule-import", test_shadowrocket_rules);
    GLib.Test.add_func ("/ssh/dns-tcp-framing-and-failure", test_dns_tcp);
    return GLib.Test.run ();
}

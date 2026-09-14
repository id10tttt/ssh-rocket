namespace Sshuttle {

    /**
     * NftManager
     * 管理 nftables 中针对 cgroup v2 与域名 IP 集合的过滤规则。
     * 支持动态 IP 集合 (proxy_ips / direct_ips) 与 DNS 局部重定向。
     * 断开连接或程序退出时彻底清理，不留残余。
     */
    public class NftManager : Object {
        public int active_port { get; set; default = 12300; }
        private bool active_ipv6 = false;

        /**
         * 检查 sshuttle 已创建当前端口对应的基础表和链。
         */
        public bool base_chains_exist (int port, bool ipv6_enabled) {
            if (Posix.geteuid () != 0) {
                try {
                    return RuntimeClient.proxy != null && RuntimeClient.proxy.base_chains_exist (port, ipv6_enabled);
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                    return false;
                }
            }
            string table_v4 = @"sshuttle-ipv4-$(port)";
            if (!this.chain_exists ("inet", table_v4, "output") ||
                !this.chain_exists ("inet", table_v4, table_v4)) {
                return false;
            }

            if (ipv6_enabled) {
                string table_v6 = @"sshuttle-ipv6-$(port)";
                if (!this.chain_exists ("inet", table_v6, "output") ||
                    !this.chain_exists ("inet", table_v6, table_v6)) {
                    return false;
                }
            }

            return true;
        }

        /**
         * 检查系统中是否已有其他 sshuttle nftables 会话。
         */
        public bool has_active_sshuttle_tables () {
            if (Posix.geteuid () != 0) {
                try {
                    return RuntimeClient.proxy != null && RuntimeClient.proxy.has_active_tables ();
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                    return false;
                }
            }
            try {
                string[] argv = { Config.NFT_PATH, "list", "tables" };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (
                    null,
                    argv,
                    null,
                    GLib.SpawnFlags.SEARCH_PATH,
                    null,
                    out stdout_text,
                    out stderr_text,
                    out exit_status
                );
                if (exit_status != 0 || stdout_text == null) {
                    return false;
                }
                return "sshuttle-ipv4-" in stdout_text || "sshuttle-ipv6-" in stdout_text;
            } catch (GLib.Error e) {
                warning ("Failed to inspect active sshuttle tables: %s", e.message);
                return false;
            }
        }

        /**
         * 向 sshuttle 的 nftables 表插入 cgroup 与域名分流规则
         */
        public bool apply_cgroup_filter (
            int port,
            bool ipv6_enabled,
            string default_policy,
            string[] direct_networks,
            DomainRule[] routing_rules
        ) {
            this.active_port = port;
            this.active_ipv6 = ipv6_enabled;
            if (Posix.geteuid () != 0) {
                string[] patterns = {};
                string[] actions = {};
                foreach (var rule in routing_rules) {
                    patterns += rule.pattern;
                    actions += rule.action;
                }
                try {
                    return RuntimeClient.proxy != null && RuntimeClient.proxy.apply_routing (
                        port, ipv6_enabled, default_policy, direct_networks, patterns, actions);
                } catch (GLib.Error e) {
                    warning ("Routing rules failed: %s", e.message);
                    return false;
                }
            }
            bool success = true;

            string table_v4 = @"sshuttle-ipv4-$(port)";
            var direct_v4 = new GLib.GenericArray<string> ();
            var proxy_v4 = new GLib.GenericArray<string> ();
            var direct_v6 = new GLib.GenericArray<string> ();
            var proxy_v6 = new GLib.GenericArray<string> ();

            foreach (var network in direct_networks) {
                this.append_network (network, "direct", direct_v4, proxy_v4, direct_v6, proxy_v6);
            }
            foreach (var rule in routing_rules) {
                this.append_network (rule.pattern, rule.action, direct_v4, proxy_v4, direct_v6, proxy_v6);
            }

            // 1. 创建用于动态域名 IP 分流的 set (带 300 秒超时)
            success = this.ensure_ip_set (table_v4, "proxy_ips", "ipv4_addr") && success;
            success = this.ensure_ip_set (table_v4, "direct_ips", "ipv4_addr") && success;

            // 2. 清理旧规则以防重复
            this.remove_cgroup_filter (port, ipv6_enabled);

            // 3. 在 output 链插入规则 (注意倒序插入以确保最终执行顺序)：
            //   1) DnsProxy 自身上游查询放行，杜绝回环
            //   2) 已勾选 App 的 DNS 查询重定向至 App 专用入口
            //   3) 其他 DNS 查询重定向至通用入口
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) output meta nfproto ipv4 udp dport 53 redirect to :15353") && success;
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) output meta nfproto ipv4 socket cgroupv2 level 1 \"sshuttle-proxy\" udp dport 53 redirect to :15355") && success;
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) output meta nfproto ipv4 udp sport 15356 return") && success;
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) output meta nfproto ipv4 udp sport 15354 return") && success;

            // 4. 在 sshuttle 子链首部插入分流与裁决规则 (倒序插入)：
            // 最终期望执行顺序：
            //   1) ip daddr @direct_ips return (直连域名/IP 集合优先放行，无论哪个 App 访问均直连)
            //   2) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port) (显式代理规则)
            //   3) socket cgroupv2 level 1 "sshuttle-proxy" meta l4proto tcp redirect to :$(port) (勾选应用未命中显式规则时全量走代理)
            //   4) [默认 direct] socket cgroupv2 level 1 != "sshuttle-proxy" return (未勾选应用未命中规则时直连)
            //   5) [默认 proxy] 继续执行 sshuttle 原有规则，使未勾选应用的其余 TCP 流量也走代理
            if (default_policy == "direct") {
                success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) socket cgroupv2 level 1 != \"sshuttle-proxy\" return") && success;
            }
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) socket cgroupv2 level 1 \"sshuttle-proxy\" meta l4proto tcp redirect to :$(port)") && success;
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port)") && success;
            for (uint i = 0; i < proxy_v4.length; i++) {
                success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr $(proxy_v4[i]) meta l4proto tcp redirect to :$(port) comment \"sshuttle-gui-network\"") && success;
            }
            success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @direct_ips return") && success;
            for (uint i = 0; i < direct_v4.length; i++) {
                success = this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr $(direct_v4[i]) return comment \"sshuttle-gui-network\"") && success;
            }

            if (ipv6_enabled) {
                string table_v6 = @"sshuttle-ipv6-$(port)";
                success = this.ensure_ip_set (table_v6, "proxy_ips", "ipv6_addr") && success;
                success = this.ensure_ip_set (table_v6, "direct_ips", "ipv6_addr") && success;
                if (default_policy == "direct") {
                    success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) socket cgroupv2 level 1 != \"sshuttle-proxy\" return") && success;
                }
                success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) socket cgroupv2 level 1 \"sshuttle-proxy\" meta l4proto tcp redirect to :$(port)") && success;
                success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) ip6 daddr @proxy_ips meta l4proto tcp redirect to :$(port)") && success;
                for (uint i = 0; i < proxy_v6.length; i++) {
                    success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) ip6 daddr $(proxy_v6[i]) meta l4proto tcp redirect to :$(port) comment \"sshuttle-gui-network\"") && success;
                }
                success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) ip6 daddr @direct_ips return") && success;
                for (uint i = 0; i < direct_v6.length; i++) {
                    success = this.run_nft_command (@"nft insert rule inet $(table_v6) $(table_v6) ip6 daddr $(direct_v6[i]) return comment \"sshuttle-gui-network\"") && success;
                }
            }

            return success;
        }

        /**
         * 动态将解析出的多个 IP 批量添加到指定集合中
         */
        public void add_ips_to_set (string[] ips, string action) {
            if (Posix.geteuid () != 0) {
                try {
                    if (RuntimeClient.proxy != null) {
                        RuntimeClient.proxy.add_ips (ips, action);
                    }
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                }
                return;
            }
            if (this.active_port <= 0 || ips.length == 0) {
                return;
            }

            var elements_v4 = new GLib.GenericArray<string> ();
            var elements_v6 = new GLib.GenericArray<string> ();
            foreach (var ip in ips) {
                string trimmed = ip.strip ();
                var address = new GLib.InetAddress.from_string (trimmed);
                if (address == null) {
                    continue;
                }
                if (address.get_family () == GLib.SocketFamily.IPV6) {
                    if (this.active_ipv6) {
                        elements_v6.add (@"$(address.to_string ()) timeout 300s");
                    }
                } else {
                    elements_v4.add (@"$(address.to_string ()) timeout 300s");
                }
            }

            string set_name = (action == "proxy") ? "proxy_ips" : "direct_ips";
            this.add_elements_to_set (@"sshuttle-ipv4-$(this.active_port)", set_name, elements_v4);
            if (this.active_ipv6) {
                this.add_elements_to_set (@"sshuttle-ipv6-$(this.active_port)", set_name, elements_v6);
            }
        }

        public void flush_ip_sets () {
            if (Posix.geteuid () != 0) {
                try {
                    if (RuntimeClient.proxy != null) {
                        RuntimeClient.proxy.flush_ips ();
                    }
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                }
                return;
            }
            this.run_nft_command (@"nft flush set inet sshuttle-ipv4-$(this.active_port) proxy_ips");
            this.run_nft_command (@"nft flush set inet sshuttle-ipv4-$(this.active_port) direct_ips");
            if (this.active_ipv6) {
                this.run_nft_command (@"nft flush set inet sshuttle-ipv6-$(this.active_port) proxy_ips");
                this.run_nft_command (@"nft flush set inet sshuttle-ipv6-$(this.active_port) direct_ips");
            }
        }

        /**
         * 动态将解析出的 IP 添加到指定集合中
         */
        public void add_ip_to_set (string ip, string action) {
            this.add_ips_to_set ({ ip }, action);
        }

        /**
         * 移除 cgroup 过滤规则
         */
        public void remove_cgroup_filter (int port, bool ipv6_enabled) {
            if (Posix.geteuid () != 0) {
                return;
            }
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15354");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15355");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15356");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15353");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "@proxy_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "@direct_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "@proxy_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "sshuttle-proxy");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "sshuttle-gui-network");

            if (ipv6_enabled) {
                this.delete_matching_rules ("inet", @"sshuttle-ipv6-$(port)", @"sshuttle-ipv6-$(port)", "@direct_ips");
                this.delete_matching_rules ("inet", @"sshuttle-ipv6-$(port)", @"sshuttle-ipv6-$(port)", "@proxy_ips");
                this.delete_matching_rules ("inet", @"sshuttle-ipv6-$(port)", @"sshuttle-ipv6-$(port)", "sshuttle-proxy");
                this.delete_matching_rules ("inet", @"sshuttle-ipv6-$(port)", @"sshuttle-ipv6-$(port)", "sshuttle-gui-network");
            }
        }

        private void append_network (
            string value,
            string action,
            GLib.GenericArray<string> direct_v4,
            GLib.GenericArray<string> proxy_v4,
            GLib.GenericArray<string> direct_v6,
            GLib.GenericArray<string> proxy_v6
        ) {
            string normalized;
            bool is_ipv6;
            if (!this.normalize_network (value, out normalized, out is_ipv6)) {
                return;
            }

            bool is_proxy = action.strip ().down () == "proxy";
            if (is_ipv6) {
                (is_proxy ? proxy_v6 : direct_v6).add (normalized);
            } else {
                (is_proxy ? proxy_v4 : direct_v4).add (normalized);
            }
        }

        private bool normalize_network (string value, out string normalized, out bool is_ipv6) {
            normalized = "";
            is_ipv6 = false;
            string[] parts = value.strip ().split ("/", 2);
            if (parts.length == 0 || parts[0] == "") {
                return false;
            }

            var address = new GLib.InetAddress.from_string (parts[0]);
            if (address == null) {
                return false;
            }

            is_ipv6 = address.get_family () == GLib.SocketFamily.IPV6;
            int max_prefix = is_ipv6 ? 128 : 32;
            int prefix = max_prefix;
            if (parts.length == 2 && (!int.try_parse (parts[1], out prefix) || prefix < 0 || prefix > max_prefix)) {
                return false;
            }

            normalized = address.to_string ();
            if (parts.length == 2) {
                normalized = @"$(normalized)/$(prefix)";
            }
            return true;
        }

        private void add_elements_to_set (string table_name, string set_name, GLib.GenericArray<string> elements) {
            if (elements.length == 0) {
                return;
            }

            var arr = new string[elements.length];
            for (uint i = 0; i < elements.length; i++) {
                arr[i] = elements[i];
            }
            string joined = string.joinv (", ", arr);
            this.run_nft_command (@"nft add element inet $(table_name) $(set_name) '{ $(joined) }'");
        }

        private bool ensure_ip_set (string table_name, string set_name, string address_type) {
            try {
                string[] argv = { Config.NFT_PATH, "list", "set", "inet", table_name, set_name };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (
                    null,
                    argv,
                    null,
                    GLib.SpawnFlags.SEARCH_PATH,
                    null,
                    out stdout_text,
                    out stderr_text,
                    out exit_status
                );
                if (exit_status == 0) {
                    return true;
                }
            } catch (GLib.Error e) {
                warning ("Failed to inspect nft set %s/%s: %s", table_name, set_name, e.message);
                return false;
            }

            return this.run_nft_command (
                @"nft add set inet $(table_name) $(set_name) '{ type $(address_type); flags timeout; }'"
            );
        }

        /**
         * 检查指定 nftables 链是否存在，并吞掉尚未创建时的预期错误输出。
         */
        private bool chain_exists (string family, string table_name, string chain_name) {
            try {
                string[] argv = { Config.NFT_PATH, "list", "chain", family, table_name, chain_name };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (
                    null,
                    argv,
                    null,
                    GLib.SpawnFlags.SEARCH_PATH,
                    null,
                    out stdout_text,
                    out stderr_text,
                    out exit_status
                );
                return exit_status == 0;
            } catch (GLib.Error e) {
                warning ("Failed to inspect nft chain %s/%s: %s", table_name, chain_name, e.message);
                return false;
            }
        }

        private void delete_matching_rules (string family, string table_name, string chain_name, string keyword) {
            try {
                string[] argv = { Config.NFT_PATH, "-a", "list", "chain", family, table_name, chain_name };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_text, out stderr_text, out exit_status);
                if (exit_status == 0 && stdout_text != null) {
                    string[] lines = stdout_text.split ("\n");
                    foreach (var line in lines) {
                        if (keyword in line && "# handle " in line) {
                            var parts = line.split ("# handle ");
                            if (parts.length >= 2) {
                                string handle = parts[1].strip ();
                                string del_cmd = @"nft delete rule $(family) $(table_name) $(chain_name) handle $(handle)";
                                this.run_nft_command (del_cmd);
                            }
                        }
                    }
                }
            } catch (GLib.Error e) {
            }
        }

        /**
         * 启用黑名单内核阻断规则：凡是在 sshuttle-block cgroup 的进程，所有外出网络在优先级 -100 直接 drop
         */
        public bool apply_blacklist_filter () {
            if (Posix.geteuid () != 0) {
                try {
                    return RuntimeClient.proxy != null && RuntimeClient.proxy.apply_blacklist ();
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                    return false;
                }
            }
            bool success = true;
            success = this.run_nft_command ("nft add table inet sshuttle-firewall") && success;
            success = this.run_nft_command ("nft 'add chain inet sshuttle-firewall output { type filter hook output priority -100; policy accept; }'") && success;
            success = this.run_nft_command ("nft 'add rule inet sshuttle-firewall output socket cgroupv2 level 1 \"sshuttle-block\" drop'") && success;
            // 被代理的应用 (如 Chrome) 遇到 UDP 443 (QUIC) 立即在 filter 链 reject，促使浏览器秒级降级为 TCP 走代理
            success = this.run_nft_command ("nft 'add rule inet sshuttle-firewall output socket cgroupv2 level 1 \"sshuttle-proxy\" udp dport 443 reject'") && success;
            return success;
        }

        public void cleanup_blacklist_filter () {
            if (Posix.geteuid () != 0) {
                return;
            }
            this.run_nft_command ("nft delete table inet sshuttle-firewall");
        }

        /**
         * 清理当前连接使用的 sshuttle nftables 表。
         */
        public void cleanup_all_sshuttle_tables (int port = 0) {
            if (Posix.geteuid () != 0) {
                try {
                    if (RuntimeClient.proxy != null) {
                        RuntimeClient.proxy.cleanup ();
                    }
                } catch (GLib.Error e) {
                    warning ("Runtime firewall operation failed: %s", e.message);
                }
                return;
            }
            this.cleanup_blacklist_filter ();

            if (port > 0) {
                this.run_nft_command (@"nft delete table inet sshuttle-ipv4-$(port)");
                this.run_nft_command (@"nft delete table inet sshuttle-ipv6-$(port)");
            }
        }

        private bool run_nft_command (string command) {
            try {
                string[] argv;
                GLib.Shell.parse_argv (command, out argv);
                argv[0] = Config.NFT_PATH;
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (
                    null,
                    argv,
                    null,
                    GLib.SpawnFlags.SEARCH_PATH,
                    null,
                    out stdout_text,
                    out stderr_text,
                    out exit_status
                );
                if (exit_status != 0 && stderr_text != null && stderr_text.strip () != "") {
                    string err_msg = stderr_text.strip ();
                    // 清理时若表原本就不存在，属于正常预期，不输出警告日志
                    if (!("delete table" in command && "No such file or directory" in err_msg)) {
                        warning ("nft command failed (code %d): %s | command: %s", exit_status, err_msg, command);
                    }
                }
                return (exit_status == 0);
            } catch (GLib.Error e) {
                warning ("nft spawn failed: %s | command: %s", e.message, command);
                return false;
            }
        }
    }
}

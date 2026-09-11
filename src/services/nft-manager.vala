namespace Sshuttle {

    /**
     * NftManager
     * 管理 nftables 中针对 cgroup v2 与域名 IP 集合的过滤规则。
     * 支持动态 IP 集合 (proxy_ips / direct_ips) 与 DNS 局部重定向。
     * 断开连接或程序退出时彻底清理，不留残余。
     */
    public class NftManager : Object {
        public int active_port { get; set; default = 12300; }

        /**
         * 向 sshuttle 的 nftables 表插入 cgroup 与域名分流规则
         */
        public bool apply_cgroup_filter (int port, bool ipv6_enabled, string default_policy = "direct") {
            this.active_port = port;
            bool success = true;

            string table_v4 = @"sshuttle-ipv4-$(port)";

            // 1. 创建用于动态域名 IP 分流的 set (带 300 秒超时)
            this.run_nft_command (@"nft add set inet $(table_v4) proxy_ips '{ type ipv4_addr; flags timeout; }'");
            this.run_nft_command (@"nft add set inet $(table_v4) direct_ips '{ type ipv4_addr; flags timeout; }'");

            // 2. 清理旧规则以防重复
            this.remove_cgroup_filter (port, ipv6_enabled);

            // 3. 在 output 链插入规则 (注意倒序插入以确保最终执行顺序)：
            //   1) udp sport 15354 return (DnsProxy 自身上游查询放行，杜绝回环)
            //   2) ip daddr @proxy_ips udp dport 443 reject (QUIC 快速 reject，促使浏览器秒级降级为 TCP)
            //   3) udp dport 53 redirect to :15353 (全局 DNS 查询重定向至本地 DnsProxy)
            this.run_nft_command (@"nft insert rule inet $(table_v4) output udp dport 53 redirect to :15353");
            this.run_nft_command (@"nft insert rule inet $(table_v4) output ip daddr @proxy_ips udp dport 443 reject");
            this.run_nft_command (@"nft insert rule inet $(table_v4) output udp sport 15354 return");

            // 4. 在 sshuttle 子链首部插入分流与裁决规则 (倒序插入)：
            // 最终期望执行顺序：
            //   1) ip daddr @direct_ips return (直连域名/IP 优先放行)
            //   2) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port) (命中代理集合的 IP 无论哪个 App 访问均走代理)
            //   3) socket cgroupv2 level 1 != "sshuttle-proxy" return (未勾选的应用默认直连放行)
            //   4) [若 default_policy == direct]: ip daddr != @proxy_ips return (已勾选的应用若默认直连则放行未指定代理的 IP)
            //   5) (sshuttle 默认规则) redirect to :$(port) (已勾选的应用走代理)
            if (default_policy == "direct") {
                this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr != @proxy_ips return");
            }
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) socket cgroupv2 level 1 != \"sshuttle-proxy\" return");
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port)");
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @direct_ips return");

            if (ipv6_enabled) {
                string table_v6 = @"sshuttle-ipv6-$(port)";
                string cmd_v6 = @"nft insert rule inet $(table_v6) output socket cgroupv2 level 1 != \"sshuttle-proxy\" return";
                this.run_nft_command (cmd_v6);
            }

            return success;
        }

        /**
         * 动态将解析出的 IP 添加到指定集合中
         */
        public void add_ip_to_set (string ip, string action) {
            if (this.active_port <= 0 || ip == "") {
                return;
            }

            string table_v4 = @"sshuttle-ipv4-$(this.active_port)";
            string set_name = (action == "proxy") ? "proxy_ips" : "direct_ips";
            string cmd = @"nft add element inet $(table_v4) $(set_name) '{ $(ip) timeout 300s }'";
            this.run_nft_command (cmd);
        }

        /**
         * 移除 cgroup 过滤规则
         */
        public void remove_cgroup_filter (int port, bool ipv6_enabled) {
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15354");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "15353");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", "output", "@proxy_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "@direct_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "@proxy_ips");
            this.delete_matching_rules ("inet", @"sshuttle-ipv4-$(port)", @"sshuttle-ipv4-$(port)", "sshuttle-proxy");

            if (ipv6_enabled) {
                this.delete_matching_rules ("inet", @"sshuttle-ipv6-$(port)", "output", "sshuttle-proxy");
            }
        }

        private void delete_matching_rules (string family, string table_name, string chain_name, string keyword) {
            try {
                string[] argv = { "nft", "-a", "list", "chain", family, table_name, chain_name };
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
        public void apply_blacklist_filter () {
            this.run_nft_command ("nft add table inet sshuttle-firewall");
            this.run_nft_command ("nft 'add chain inet sshuttle-firewall output { type filter hook output priority -100; policy accept; }'");
            this.run_nft_command ("nft 'add rule inet sshuttle-firewall output socket cgroupv2 level 1 \"sshuttle-block\" drop'");
        }

        public void cleanup_blacklist_filter () {
            this.run_nft_command ("nft delete table inet sshuttle-firewall");
        }

        /**
         * 彻底清理指定端口或所有残留的 sshuttle nftables 表
         */
        public void cleanup_all_sshuttle_tables (int port = 0) {
            this.cleanup_blacklist_filter ();

            if (port > 0) {
                this.run_nft_command (@"nft delete table inet sshuttle-ipv4-$(port)");
                this.run_nft_command (@"nft delete table inet sshuttle-ipv6-$(port)");
            }

            // 扫描所有残留的 sshuttle 表并清除
            try {
                string[] argv = { "nft", "list", "tables" };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_text, out stderr_text, out exit_status);
                if (exit_status == 0 && stdout_text != null) {
                    string[] lines = stdout_text.split ("\n");
                    foreach (var line in lines) {
                        string trimmed = line.strip ();
                        if (trimmed.has_prefix ("table ")) {
                            string[] tokens = trimmed.split (" ");
                            if (tokens.length >= 3) {
                                string family = tokens[1];
                                string tbl = tokens[2];
                                if (tbl.has_prefix ("sshuttle-ipv4-") || tbl.has_prefix ("sshuttle-ipv6-") || tbl == "sshuttle-firewall") {
                                    this.run_nft_command (@"nft delete table $(family) $(tbl)");
                                }
                            }
                        }
                    }
                }
            } catch (GLib.Error e) {
            }
        }

        private bool run_nft_command (string command) {
            try {
                string[] argv;
                GLib.Shell.parse_argv (command, out argv);
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
                return (exit_status == 0);
            } catch (GLib.Error e) {
                return false;
            }
        }
    }
}

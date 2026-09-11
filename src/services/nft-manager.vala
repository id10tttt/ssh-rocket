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
            //   2) udp dport 53 redirect to :15353 (全局 DNS 查询重定向至本地 DnsProxy)
            this.run_nft_command (@"nft insert rule inet $(table_v4) output udp dport 53 redirect to :15353");
            this.run_nft_command (@"nft insert rule inet $(table_v4) output udp sport 15354 return");

            // 4. 在 sshuttle 子链首部插入分流与裁决规则 (倒序插入)：
            // 最终期望执行顺序：
            //   1) ip daddr @direct_ips return (直连域名/IP 集合优先放行，无论哪个 App 访问均直连)
            //   2) socket cgroupv2 level 1 "sshuttle-proxy" meta l4proto tcp redirect to :$(port) (勾选应用的所有其他 TCP 流量全量走代理，包括硬编码 IP / 视频流)
            //   3) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port) (未勾选应用命中代理域名 IP 集合时走代理)
            //   4) socket cgroupv2 level 1 != "sshuttle-proxy" return (未勾选应用其余普通流量直接直连放行，不走代理)
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) socket cgroupv2 level 1 != \"sshuttle-proxy\" return");
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @proxy_ips meta l4proto tcp redirect to :$(port)");
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) socket cgroupv2 level 1 \"sshuttle-proxy\" meta l4proto tcp redirect to :$(port)");
            this.run_nft_command (@"nft insert rule inet $(table_v4) $(table_v4) ip daddr @direct_ips return");

            if (ipv6_enabled) {
                string table_v6 = @"sshuttle-ipv6-$(port)";
                string cmd_v6 = @"nft insert rule inet $(table_v6) output socket cgroupv2 level 1 != \"sshuttle-proxy\" return";
                this.run_nft_command (cmd_v6);
            }

            return success;
        }

        /**
         * 动态将解析出的多个 IP 批量添加到指定集合中
         */
        public void add_ips_to_set (string[] ips, string action) {
            if (this.active_port <= 0 || ips.length == 0) {
                return;
            }

            var elements = new GLib.GenericArray<string> ();
            foreach (var ip in ips) {
                string trimmed = ip.strip ();
                if (trimmed != "") {
                    elements.add (@"$(trimmed) timeout 300s");
                }
            }

            if (elements.length == 0) {
                return;
            }

            var arr = new string[elements.length];
            for (uint i = 0; i < elements.length; i++) {
                arr[i] = elements[i];
            }

            string joined = string.joinv (", ", arr);
            string table_v4 = @"sshuttle-ipv4-$(this.active_port)";
            string set_name = (action == "proxy") ? "proxy_ips" : "direct_ips";
            string cmd = @"nft add element inet $(table_v4) $(set_name) '{ $(joined) }'";
            this.run_nft_command (cmd);
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
            // 被代理的应用 (如 Chrome) 遇到 UDP 443 (QUIC) 立即在 filter 链 reject，促使浏览器秒级降级为 TCP 走代理
            this.run_nft_command ("nft 'add rule inet sshuttle-firewall output socket cgroupv2 level 1 \"sshuttle-proxy\" udp dport 443 reject'");
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
                if (exit_status != 0 && stderr_text != null && stderr_text.strip () != "") {
                    warning ("nft command failed (code %d): %s | command: %s", exit_status, stderr_text.strip (), command);
                }
                return (exit_status == 0);
            } catch (GLib.Error e) {
                warning ("nft spawn failed: %s | command: %s", e.message, command);
                return false;
            }
        }
    }
}

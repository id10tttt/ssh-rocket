namespace Sshuttle {

    /**
     * NftManager
     * 管理 nftables 中针对 cgroup v2 的应用过滤规则。
     * 当启用按应用代理时，向 sshuttle 动态创建的 output 链首部插入规则：
     *   socket cgroupv2 level 1 != "sshuttle-proxy" return
     * 确保非选定软件的数据包直接放行（不跳转到 sshuttle 重定向链）。
     * 断开连接或程序退出时彻底清理，不留残余。
     */
    public class NftManager : Object {

        /**
         * 向 sshuttle 的 nftables 表插入 cgroup 过滤规则
         */
        public bool apply_cgroup_filter (int port, bool ipv6_enabled) {
            bool success = true;

            string table_v4 = @"sshuttle-ipv4-$(port)";
            string cmd_v4 = @"nft insert rule ip $(table_v4) output socket cgroupv2 level 1 != \"sshuttle-proxy\" return";
            if (!this.run_nft_command (cmd_v4)) {
                warning ("Failed to insert IPv4 cgroup filter rule");
                success = false;
            }

            if (ipv6_enabled) {
                string table_v6 = @"sshuttle-ipv6-$(port)";
                string cmd_v6 = @"nft insert rule ip6 $(table_v6) output socket cgroupv2 level 1 != \"sshuttle-proxy\" return";
                if (!this.run_nft_command (cmd_v6)) {
                    warning ("Failed to insert IPv6 cgroup filter rule");
                }
            }

            return success;
        }

        /**
         * 移除 cgroup 过滤规则（在运行时动态恢复为全局代理）
         */
        public void remove_cgroup_filter (int port, bool ipv6_enabled) {
            this.delete_cgroup_rule_from_table ("ip", @"sshuttle-ipv4-$(port)");
            if (ipv6_enabled) {
                this.delete_cgroup_rule_from_table ("ip6", @"sshuttle-ipv6-$(port)");
            }
        }

        private void delete_cgroup_rule_from_table (string family, string table_name) {
            try {
                string[] argv = { "nft", "-a", "list", "chain", family, table_name, "output" };
                string stdout_text;
                string stderr_text;
                int exit_status;
                GLib.Process.spawn_sync (null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null, out stdout_text, out stderr_text, out exit_status);
                if (exit_status == 0 && stdout_text != null) {
                    string[] lines = stdout_text.split ("\n");
                    foreach (var line in lines) {
                        if ("sshuttle-proxy" in line && "# handle " in line) {
                            var parts = line.split ("# handle ");
                            if (parts.length >= 2) {
                                string handle = parts[1].strip ();
                                string del_cmd = @"nft delete rule $(family) $(table_name) output handle $(handle)";
                                this.run_nft_command (del_cmd);
                            }
                        }
                    }
                }
            } catch (GLib.Error e) {
                // 忽略不存在或查询失败
            }
        }

        /**
         * 彻底清理指定端口或所有残留的 sshuttle nftables 表
         */
        public void cleanup_all_sshuttle_tables (int port = 0) {
            if (port > 0) {
                this.run_nft_command (@"nft delete table ip sshuttle-ipv4-$(port)");
                this.run_nft_command (@"nft delete table ip6 sshuttle-ipv6-$(port)");
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
                        // 示例: table ip sshuttle-ipv4-12300
                        if (trimmed.has_prefix ("table ")) {
                            string[] tokens = trimmed.split (" ");
                            if (tokens.length >= 3) {
                                string family = tokens[1];
                                string tbl = tokens[2];
                                if (tbl.has_prefix ("sshuttle-ipv4-") || tbl.has_prefix ("sshuttle-ipv6-")) {
                                    this.run_nft_command (@"nft delete table $(family) $(tbl)");
                                }
                            }
                        }
                    }
                }
            } catch (GLib.Error e) {
                // nft 可能未运行或无规则
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

namespace Sshuttle {

    /**
     * DnsProxy
     * 轻量 DNS 分流器，仅处理被代理 cgroup 进程的 DNS 查询。
     * 解析请求的 QNAME 域名并匹配 Zero Omega 域名通配符规则：
     * - 若匹配到 Proxy：解析后将 IP 加入 nftables proxy_ips 集合（走代理）
     * - 若匹配到 Direct：解析后将 IP 加入 nftables direct_ips 集合（走直连）
     * - 若未匹配：依 default_policy 决定
     */
    public class DnsProxy : Object {
        public const uint16 DNS_PORT = 15353;
        private ConfigManager config_manager;
        private NftManager nft_manager;
        private GLib.Socket? server_socket = null;
        private bool running = false;
        private GLib.Thread<void*>? worker_thread = null;

        public DnsProxy (ConfigManager config_manager, NftManager nft_manager) {
            this.config_manager = config_manager;
            this.nft_manager = nft_manager;
        }

        public bool start () {
            if (this.running) {
                return true;
            }

            try {
                this.server_socket = new GLib.Socket (
                    GLib.SocketFamily.IPV4,
                    GLib.SocketType.DATAGRAM,
                    GLib.SocketProtocol.UDP
                );

                var inet_addr = new GLib.InetAddress.from_string ("127.0.0.1");
                var sockaddr = new GLib.InetSocketAddress (inet_addr, DNS_PORT);
                this.server_socket.bind (sockaddr, true);
                this.server_socket.set_timeout (1); // 1秒超时，便于循环退出

                this.running = true;
                this.worker_thread = new GLib.Thread<void*> ("dns-worker", this.run_loop);
                return true;
            } catch (GLib.Error e) {
                warning ("Failed to start DnsProxy on port %u: %s", DNS_PORT, e.message);
                this.running = false;
                return false;
            }
        }

        public void stop () {
            if (!this.running) {
                return;
            }

            this.running = false;
            if (this.server_socket != null) {
                try {
                    this.server_socket.close ();
                } catch (GLib.Error e) {
                }
                this.server_socket = null;
            }

            if (this.worker_thread != null) {
                this.worker_thread.join ();
                this.worker_thread = null;
            }
        }

        private void* run_loop () {
            uint8 buffer[2048];
            while (this.running && this.server_socket != null) {
                try {
                    GLib.SocketAddress client_addr;
                    ssize_t size = this.server_socket.receive_from (out client_addr, buffer);
                    if (size > 0 && this.running) {
                        uint8[] packet = new uint8[size];
                        GLib.Memory.copy (packet, buffer, size);
                        this.handle_dns_query (packet, client_addr);
                    }
                } catch (GLib.Error e) {
                    // 超时或关闭，正常循环检测
                }
            }
            return null;
        }

        private void handle_dns_query (uint8[] query_packet, GLib.SocketAddress client_addr) {
            string? domain = parse_qname (query_packet);
            string action = this.resolve_action_for_domain (domain);

            // 选择上游 DNS 服务器
            string upstream_ip = (action == "proxy") ? "8.8.8.8" : "127.0.0.53";

            try {
                var forward_socket = new GLib.Socket (
                    GLib.SocketFamily.IPV4,
                    GLib.SocketType.DATAGRAM,
                    GLib.SocketProtocol.UDP
                );
                forward_socket.set_timeout (3);

                var up_inet = new GLib.InetAddress.from_string (upstream_ip);
                var up_addr = new GLib.InetSocketAddress (up_inet, 53);

                forward_socket.send_to (up_addr, query_packet);

                uint8 resp_buf[2048];
                GLib.SocketAddress resp_src;
                ssize_t resp_len = forward_socket.receive_from (out resp_src, resp_buf);

                if (resp_len > 0) {
                    uint8[] resp_packet = new uint8[resp_len];
                    GLib.Memory.copy (resp_packet, resp_buf, resp_len);

                    // 提取响应中的 IP 并加入 nftables 集合
                    var ips = parse_answer_ips (resp_packet);
                    foreach (var ip in ips) {
                        this.nft_manager.add_ip_to_set (ip, action);
                    }

                    // 回发给客户端
                    if (this.server_socket != null) {
                        this.server_socket.send_to (client_addr, resp_packet);
                    }
                }

                forward_socket.close ();
            } catch (GLib.Error e) {
                // 如果本地 DNS 超时，尝试备用公共 DNS
                if (upstream_ip == "127.0.0.53") {
                    this.forward_fallback (query_packet, client_addr, "223.5.5.5", action);
                }
            }
        }

        private void forward_fallback (uint8[] query_packet, GLib.SocketAddress client_addr, string fallback_ip, string action) {
            try {
                var forward_socket = new GLib.Socket (
                    GLib.SocketFamily.IPV4,
                    GLib.SocketType.DATAGRAM,
                    GLib.SocketProtocol.UDP
                );
                forward_socket.set_timeout (3);

                var up_inet = new GLib.InetAddress.from_string (fallback_ip);
                var up_addr = new GLib.InetSocketAddress (up_inet, 53);

                forward_socket.send_to (up_addr, query_packet);

                uint8 resp_buf[2048];
                GLib.SocketAddress resp_src;
                ssize_t resp_len = forward_socket.receive_from (out resp_src, resp_buf);

                if (resp_len > 0) {
                    uint8[] resp_packet = new uint8[resp_len];
                    GLib.Memory.copy (resp_packet, resp_buf, resp_len);

                    var ips = parse_answer_ips (resp_packet);
                    foreach (var ip in ips) {
                        this.nft_manager.add_ip_to_set (ip, action);
                    }

                    if (this.server_socket != null) {
                        this.server_socket.send_to (client_addr, resp_packet);
                    }
                }
                forward_socket.close ();
            } catch (GLib.Error e) {
            }
        }

        public string resolve_action_for_domain (string? domain) {
            if (domain == null || domain == "") {
                return this.config_manager.get_domain_default_policy ();
            }

            var rules = this.config_manager.get_domain_rules ();
            foreach (var rule in rules) {
                if (rule.matches (domain)) {
                    return rule.action;
                }
            }

            return this.config_manager.get_domain_default_policy ();
        }

        public static string? parse_qname (uint8[] data) {
            if (data.length < 13) {
                return null;
            }

            int idx = 12;
            var parts = new GLib.GenericArray<string> ();

            while (idx < data.length) {
                uint8 len = data[idx];
                if (len == 0) {
                    break;
                }
                if ((len & 0xC0) == 0xC0) {
                    break;
                }

                idx++;
                if (idx + len > data.length) {
                    return null;
                }

                uint8[] label = new uint8[len + 1];
                GLib.Memory.copy (label, ((uint8*) data) + idx, len);
                label[len] = 0;
                parts.add ((string) label);
                idx += len;
            }

            if (parts.length == 0) {
                return null;
            }

            var arr = new string[parts.length];
            for (uint i = 0; i < parts.length; i++) {
                arr[i] = parts[i];
            }
            return string.joinv (".", arr);
        }

        public static string[] parse_answer_ips (uint8[] data) {
            var ips = new GLib.GenericArray<string> ();
            if (data.length < 12) {
                return new string[0];
            }

            int ancount = (data[6] << 8) | data[7];
            if (ancount <= 0) {
                return new string[0];
            }

            // 跳过 Question
            int idx = 12;
            while (idx < data.length && data[idx] != 0) {
                if ((data[idx] & 0xC0) == 0xC0) {
                    idx += 2;
                    break;
                }
                idx += data[idx] + 1;
            }
            if (idx < data.length && data[idx] == 0) {
                idx++;
            }
            idx += 4; // qtype + qclass

            // 遍历 Answer
            for (int a = 0; a < ancount; a++) {
                if (idx >= data.length) {
                    break;
                }

                // 处理名称
                if ((data[idx] & 0xC0) == 0xC0) {
                    idx += 2;
                } else {
                    while (idx < data.length && data[idx] != 0) {
                        idx += data[idx] + 1;
                    }
                    if (idx < data.length) {
                        idx++;
                    }
                }

                if (idx + 10 > data.length) {
                    break;
                }

                int atype = (data[idx] << 8) | data[idx + 1];
                int rdlen = (data[idx + 8] << 8) | data[idx + 9];
                idx += 10;

                // Type 1 = A 记录 (IPv4)
                if (atype == 1 && rdlen == 4 && idx + 4 <= data.length) {
                    string ip = @"$(data[idx]).$(data[idx + 1]).$(data[idx + 2]).$(data[idx + 3])";
                    ips.add (ip);
                }

                idx += rdlen;
            }

            var arr = new string[ips.length];
            for (uint i = 0; i < ips.length; i++) {
                arr[i] = ips[i];
            }
            return arr;
        }
    }
}

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
        public const uint16 FORWARD_PORT = 15354;
        public uint16 remote_dns_port { get; set; default = 0; }
        public signal void dns_resolved (string domain, string action, string[] ips);
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

        private uint8[]? query_udp (string server_ip, uint16 server_port, uint8[] query_packet) {
            try {
                var s = new GLib.Socket (GLib.SocketFamily.IPV4, GLib.SocketType.DATAGRAM, GLib.SocketProtocol.UDP);
                s.set_timeout (2);
                var bind_addr = new GLib.InetSocketAddress (new GLib.InetAddress.from_string ("127.0.0.1"), FORWARD_PORT);
                s.bind (bind_addr, true);

                var target_addr = new GLib.InetSocketAddress (new GLib.InetAddress.from_string (server_ip), server_port);
                s.send_to (target_addr, query_packet);

                uint8 buf[2048];
                GLib.SocketAddress src_addr;
                ssize_t len = s.receive_from (out src_addr, buf);
                s.close ();

                if (len > 0) {
                    uint8[] resp = new uint8[len];
                    GLib.Memory.copy (resp, buf, len);
                    return resp;
                }
            } catch (GLib.Error e) {
            }
            return null;
        }

        private uint8[]? query_tcp (string server_ip, uint16 server_port, uint8[] query_packet) {
            try {
                var s = new GLib.Socket (GLib.SocketFamily.IPV4, GLib.SocketType.STREAM, GLib.SocketProtocol.TCP);
                s.set_timeout (3);
                var target_addr = new GLib.InetSocketAddress (new GLib.InetAddress.from_string (server_ip), server_port);
                s.connect (target_addr);

                // RFC 1035: 2 字节大端长度前缀
                uint16 len = (uint16) query_packet.length;
                uint8 len_buf[2];
                len_buf[0] = (uint8) ((len >> 8) & 0xff);
                len_buf[1] = (uint8) (len & 0xff);

                s.send (len_buf);
                s.send (query_packet);

                uint8 resp_len_buf[2];
                ssize_t r1 = s.receive (resp_len_buf);
                if (r1 < 2) {
                    s.close ();
                    return null;
                }

                uint16 resp_len = (uint16) ((resp_len_buf[0] << 8) | resp_len_buf[1]);
                if (resp_len == 0 || resp_len > 4096) {
                    s.close ();
                    return null;
                }

                uint8[] resp = new uint8[resp_len];
                size_t total = 0;
                while (total < resp_len) {
                    uint8 chunk[2048];
                    ssize_t r = s.receive (chunk);
                    if (r <= 0) {
                        break;
                    }
                    for (int i = 0; i < r && total + i < resp_len; i++) {
                        resp[total + i] = chunk[i];
                    }
                    total += r;
                }
                s.close ();

                if (total == resp_len) {
                    return resp;
                }
            } catch (GLib.Error e) {
            }
            return null;
        }

        private void handle_dns_query (uint8[] query_packet, GLib.SocketAddress client_addr) {
            uint16 qtype;
            string? domain = parse_qname_and_type (query_packet, out qtype);
            string action = this.resolve_action_for_domain (domain);

            // 检查当前节点是否启用了 IPv6 代理
            bool ipv6_enabled = false;
            var active_profile = this.config_manager.get_active_profile ();
            if (active_profile != null) {
                ipv6_enabled = active_profile.ipv6;
            }

            // 当域名走代理时，过滤 AAAA (IPv6, 28) 与 HTTPS (Type 65, RFC 9460)：
            // 1) AAAA: 若未开启 IPv6 代理，返回 NOERROR 空响应，促使浏览器秒级切换 IPv4
            // 2) HTTPS RR (65): 过滤返回空响应，防止现代 Chrome 尝试 ECH (加密 SNI) 或 QUIC 导致 ERR_FAILED
            if (action == "proxy" && ((qtype == 28 && !ipv6_enabled) || qtype == 65)) {
                var empty_resp = build_empty_noerror_response (query_packet);
                if (this.server_socket != null) {
                    try {
                        this.server_socket.send_to (client_addr, empty_resp);
                    } catch (GLib.Error e) {
                    }
                }
                return;
            }

            uint8[]? resp_packet = null;

            if (action == "proxy") {
                // 1. 优先尝试 sshuttle 本地隧道 DNS (无污染高速解析)
                if (this.remote_dns_port > 0) {
                    resp_packet = this.query_udp ("127.0.0.1", this.remote_dns_port, query_packet);
                }
                // 2. 若隧道 DNS 尚未就绪，通过 TCP 向 8.8.8.8 查询 (TCP 自动走 sshuttle 隧道代理)
                if (resp_packet == null) {
                    resp_packet = this.query_tcp ("8.8.8.8", 53, query_packet);
                }
            } else {
                // 直连域名：本地系统 DNS 优先
                resp_packet = this.query_udp ("127.0.0.53", 53, query_packet);
                if (resp_packet == null) {
                    resp_packet = this.query_udp ("223.5.5.5", 53, query_packet);
                }
            }

            if (resp_packet != null) {
                // 提取解析所得 IP 写入 nftables 对应集合
                var ips = parse_answer_ips (resp_packet);
                foreach (var ip in ips) {
                    this.nft_manager.add_ip_to_set (ip, action);
                }

                if (domain != null && domain != "") {
                    this.dns_resolved (domain, action, ips);
                }

                // 回发客户端
                if (this.server_socket != null) {
                    try {
                        this.server_socket.send_to (client_addr, resp_packet);
                    } catch (GLib.Error e) {
                    }
                }
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
            uint16 qtype;
            return parse_qname_and_type (data, out qtype);
        }

        public static string? parse_qname_and_type (uint8[] data, out uint16 qtype) {
            qtype = 0;
            if (data.length < 13) {
                return null;
            }

            int idx = 12;
            var parts = new GLib.GenericArray<string> ();

            while (idx < data.length) {
                uint8 len = data[idx];
                if (len == 0) {
                    idx++;
                    break;
                }
                if ((len & 0xC0) == 0xC0) {
                    idx += 2;
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

            if (idx + 2 <= data.length) {
                qtype = (uint16) ((data[idx] << 8) | data[idx + 1]);
            }

            var arr = new string[parts.length];
            for (uint i = 0; i < parts.length; i++) {
                arr[i] = parts[i];
            }
            return string.joinv (".", arr);
        }

        public static uint8[] build_empty_noerror_response (uint8[] query_packet) {
            if (query_packet.length < 12) {
                return query_packet;
            }

            uint8[] resp = new uint8[query_packet.length];
            GLib.Memory.copy (resp, query_packet, query_packet.length);

            // Flags: 0x8180 (Response, Opcode=0, AA=0, TC=0, RD=1, RA=1, RCODE=0 NOERROR)
            resp[2] = 0x81;
            resp[3] = 0x80;

            // QDCOUNT: 1
            resp[4] = 0x00;
            resp[5] = 0x01;

            // ANCOUNT: 0
            resp[6] = 0x00;
            resp[7] = 0x00;

            // NSCOUNT: 0
            resp[8] = 0x00;
            resp[9] = 0x00;

            // ARCOUNT: 0
            resp[10] = 0x00;
            resp[11] = 0x00;

            return resp;
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

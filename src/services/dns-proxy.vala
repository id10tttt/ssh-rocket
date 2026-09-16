namespace Sshuttle {

    public class DnsResolutionEntry : Object {
        public string domain { get; private set; }
        public string[] ips { get; private set; }
        public int64 resolved_at { get; private set; }

        public DnsResolutionEntry (string domain, string[] ips) {
            this.domain = domain;
            this.ips = ips;
            this.resolved_at = GLib.get_monotonic_time ();
        }
    }

    /**
     * DnsProxy
     * 轻量 DNS 分流器，仅处理被代理 cgroup 进程的 DNS 查询。
     * 解析请求的 QNAME 域名并匹配自定义或 Shadowrocket 规则：
     * - 若匹配到 Proxy：解析后将 IP 加入 nftables proxy_ips 集合（走代理）
     * - 若匹配到 Direct：解析后将 IP 加入 nftables direct_ips 集合（走直连）
     * - 若匹配到 Reject：返回 NXDOMAIN 阻断请求
     * - 若未匹配：勾选的应用走代理，其他应用使用配置默认策略
     */
    public class DnsProxy : Object {
        public const uint16 DNS_PORT = 15353;
        public const uint16 FORWARD_PORT = 15354;
        public const uint16 APP_DNS_PORT = 15355;
        public const uint16 APP_FORWARD_PORT = 15356;
        public uint16 remote_dns_port { get; set; default = 0; }
        public signal void dns_resolved (string domain, string action, string[] ips);
        private ConfigManager config_manager;
        private NftManager nft_manager;
        private GLib.Socket? server_socket = null;
        private GLib.Socket? app_server_socket = null;
        private bool running = false;
        private GLib.Thread<void*>? worker_thread = null;
        private GLib.Thread<void*>? app_worker_thread = null;
        private GLib.HashTable<string, DnsResolutionEntry> resolution_cache;
        private GLib.Mutex resolution_cache_mutex;

        public DnsProxy (ConfigManager config_manager, NftManager nft_manager) {
            this.config_manager = config_manager;
            this.nft_manager = nft_manager;
            this.resolution_cache = new GLib.HashTable<string, DnsResolutionEntry> (GLib.str_hash, GLib.str_equal);
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

                this.app_server_socket = new GLib.Socket (
                    GLib.SocketFamily.IPV4,
                    GLib.SocketType.DATAGRAM,
                    GLib.SocketProtocol.UDP
                );
                var app_sockaddr = new GLib.InetSocketAddress (inet_addr, APP_DNS_PORT);
                this.app_server_socket.bind (app_sockaddr, true);
                this.app_server_socket.set_timeout (1);

                this.running = true;
                this.worker_thread = new GLib.Thread<void*> ("dns-worker", () => {
                    return this.run_loop (this.server_socket, false, FORWARD_PORT);
                });
                this.app_worker_thread = new GLib.Thread<void*> ("dns-app-worker", () => {
                    return this.run_loop (this.app_server_socket, true, APP_FORWARD_PORT);
                });
                return true;
            } catch (GLib.Error e) {
                warning ("Failed to start DnsProxy on port %u: %s", DNS_PORT, e.message);
                this.running = false;
                if (this.server_socket != null) {
                    try {
                        this.server_socket.close ();
                    } catch (GLib.Error close_error) {
                    }
                    this.server_socket = null;
                }
                if (this.app_server_socket != null) {
                    try {
                        this.app_server_socket.close ();
                    } catch (GLib.Error close_error) {
                    }
                    this.app_server_socket = null;
                }
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
            if (this.app_server_socket != null) {
                try {
                    this.app_server_socket.close ();
                } catch (GLib.Error e) {
                }
                this.app_server_socket = null;
            }

            if (this.worker_thread != null) {
                this.worker_thread.join ();
                this.worker_thread = null;
            }
            if (this.app_worker_thread != null) {
                this.app_worker_thread.join ();
                this.app_worker_thread = null;
            }
            this.resolution_cache_mutex.lock ();
            this.resolution_cache.remove_all ();
            this.resolution_cache_mutex.unlock ();
        }

        public void rebuild_routing_sets () {
            var expired_domains = new GLib.GenericArray<string> ();
            int64 now = GLib.get_monotonic_time ();

            this.resolution_cache_mutex.lock ();
            this.nft_manager.flush_ip_sets ();
            var iter = GLib.HashTableIter<string, DnsResolutionEntry> (this.resolution_cache);
            string domain;
            DnsResolutionEntry entry;
            while (iter.next (out domain, out entry)) {
                if (now - entry.resolved_at <= 300 * GLib.TimeSpan.SECOND) {
                    bool matched;
                    string action = this.resolve_action_for_domain (entry.domain, out matched);
                    if (matched) {
                        this.nft_manager.add_ips_to_set (entry.ips, action);
                    }
                } else {
                    expired_domains.add (domain);
                }
            }
            for (uint i = 0; i < expired_domains.length; i++) {
                this.resolution_cache.remove (expired_domains[i]);
            }
            this.resolution_cache_mutex.unlock ();
        }

        private void* run_loop (GLib.Socket? source_socket, bool app_proxy_default, uint16 forward_port) {
            if (source_socket == null) {
                return null;
            }
            uint8 buffer[2048];
            while (this.running) {
                try {
                    GLib.SocketAddress client_addr;
                    ssize_t size = source_socket.receive_from (out client_addr, buffer);
                    if (size > 0 && this.running) {
                        uint8[] packet = new uint8[size];
                        GLib.Memory.copy (packet, buffer, size);
                        this.handle_dns_query (packet, client_addr, source_socket, app_proxy_default, forward_port);
                    }
                } catch (GLib.Error e) {
                    // 超时或关闭，正常循环检测
                }
            }
            return null;
        }

        private uint8[]? query_udp (string server_ip, uint16 server_port, uint8[] query_packet, uint16 forward_port) {
            GLib.Socket? s = null;
            try {
                s = new GLib.Socket (GLib.SocketFamily.IPV4, GLib.SocketType.DATAGRAM, GLib.SocketProtocol.UDP);
                s.set_timeout (5);
                var bind_addr = new GLib.InetSocketAddress (new GLib.InetAddress.from_string ("127.0.0.1"), forward_port);
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
                if (s != null) {
                    try {
                        s.close ();
                    } catch (GLib.Error e2) {
                    }
                }
            }
            return null;
        }

        /** DNS 使用两字节长度帧，通过 SSH 本地端口转发访问远端解析器。 */
        public static uint8[]? query_tcp (uint16 port, uint8[] packet) {
            try {
                var client = new GLib.SocketClient ();
                client.timeout = 5;
                var connection = client.connect_to_host ("127.0.0.1", port);
                try {
                    uint8[] frame = new uint8[packet.length + 2];
                    frame[0] = (uint8) (packet.length >> 8);
                    frame[1] = (uint8) packet.length;
                    GLib.Memory.copy ((uint8*) frame + 2, packet, packet.length);
                    size_t count;
                    connection.output_stream.write_all (frame, out count);
                    uint8[] header = new uint8[2];
                    connection.input_stream.read_all (header, out count);
                    if (count != 2) return null;
                    int length = ((int) header[0] << 8) | header[1];
                    if (length < 12) return null;
                    uint8[] response = new uint8[length];
                    connection.input_stream.read_all (response, out count);
                    if (count != length || packet.length < 2 || response[0] != packet[0] || response[1] != packet[1]) return null;
                    return response;
                } finally {
                    try { connection.close (null); } catch (GLib.Error e) {}
                }
            } catch (GLib.Error e) {
                return null;
            }
        }

        /** 通过 SSH DNS 转发解析固定域名，用于连接就绪检查。 */
        public static bool probe_remote_dns (uint16 port) {
            uint8[] query = {
                0x53, 0x52, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
                07, 'e', 'x', 'a', 'm', 'p', 'l', 'e',
                03, 'c', 'o', 'm', 0x00,
                0x00, 0x01, 0x00, 0x01
            };
            var response = query_tcp (port, query);
            return response != null && parse_answer_ips (response).length > 0;
        }

        private void handle_dns_query (
            uint8[] query_packet,
            GLib.SocketAddress client_addr,
            GLib.Socket response_socket,
            bool app_proxy_default,
            uint16 forward_port
        ) {
            uint16 qtype;
            string? domain = parse_qname_and_type (query_packet, out qtype);
            bool matched = false;
            string action = this.config_manager.resolve_traffic_action (
                domain, app_proxy_default, out matched
            );

            if (action == "reject") {
                var rejected_response = build_error_response (query_packet, 3);
                try {
                    response_socket.send_to (client_addr, rejected_response);
                } catch (GLib.Error e) {
                }
                if (domain != null && domain != "") {
                    string rejected_domain = domain;
                    GLib.Idle.add (() => {
                        this.dns_resolved (rejected_domain, "reject", {});
                        return GLib.Source.REMOVE;
                    });
                }
                return;
            }

            var settings = this.config_manager.get_network_settings ();
            bool ipv6_enabled = settings.ipv6;

            // 当域名走代理时，过滤 AAAA (IPv6, 28) 与 HTTPS (Type 65, RFC 9460)：
            // 1) AAAA: 若未开启 IPv6 代理，返回 NOERROR 空响应，促使浏览器秒级切换 IPv4
            // 2) HTTPS RR (65): 过滤返回空响应，防止现代 Chrome 尝试 ECH (加密 SNI) 或 QUIC 导致 ERR_FAILED
            if (action == "proxy" && ((qtype == 28 && !ipv6_enabled) || qtype == 65)) {
                var empty_resp = build_empty_noerror_response (query_packet);
                try {
                    response_socket.send_to (client_addr, empty_resp);
                } catch (GLib.Error e) {
                }
                return;
            }

            uint8[]? resp_packet = null;

            if (action == "proxy") {
                // 代理规则只允许经 SSH 的远端 TCP DNS 查询，避免失败时泄漏到本地网络。
                if (this.remote_dns_port > 0) {
                    resp_packet = query_tcp (this.remote_dns_port, query_packet);
                } else if (!settings.dns) {
                    // 配置明确关闭远端 DNS 时保留本地解析，否则域名规则与应用联网均无法工作。
                    resp_packet = this.query_udp ("127.0.0.53", 53, query_packet, forward_port);
                    if (resp_packet == null) {
                        resp_packet = this.query_udp ("223.5.5.5", 53, query_packet, forward_port);
                    }
                }
            } else {
                // 直连域名：本地系统 DNS 优先
                resp_packet = this.query_udp ("127.0.0.53", 53, query_packet, forward_port);
                if (resp_packet == null) {
                    resp_packet = this.query_udp ("223.5.5.5", 53, query_packet, forward_port);
                }
            }

            if (resp_packet != null) {
                // 提取解析所得 IP 批量写入 nftables 对应集合
                var ips = parse_answer_ips (resp_packet);
                if (ips.length > 0) {
                    if (domain != null && domain != "") {
                        this.resolution_cache_mutex.lock ();
                        this.resolution_cache.insert (domain, new DnsResolutionEntry (domain, ips));
                        if (matched) {
                            // 显式匹配规则：direct 为直连白名单例外（优先放行），proxy 为显式定向代理（未勾选应用也走代理）
                            this.nft_manager.add_ips_to_set (ips, action);
                        }
                        this.resolution_cache_mutex.unlock ();
                    }
                    // 未匹配规则时不写入全局 IP 集合，交由应用 cgroup 与配置默认策略裁决。
                    // 否则 direct_ips 在 nftables 首部优先放行，会导致已勾选 App 的未匹配域名被错误放行走直连。
                    // 不加入 direct_ips 时，已勾选 App 由 cgroup 规则全量代理，未勾选 App 按默认策略处理。
                }

                if (domain != null && domain != "") {
                    string d = domain;
                    string a = action;
                    string[] ips_copy = ips;
                    GLib.Idle.add (() => {
                        this.dns_resolved (d, a, ips_copy);
                        return GLib.Source.REMOVE;
                    });
                }

                // 回发客户端
                try {
                    response_socket.send_to (client_addr, resp_packet);
                } catch (GLib.Error e) {
                }
            }
        }

        public string resolve_action_for_domain (string? domain, out bool matched = null) {
            matched = false;
            if (domain == null || domain == "") {
                return this.config_manager.get_domain_default_policy ();
            }

            return this.config_manager.resolve_domain_action (domain, out matched);
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
            return build_error_response (query_packet, 0);
        }

        /** 保留 DNS Question 并构造指定 RCODE 的无应答响应。 */
        public static uint8[] build_error_response (uint8[] query_packet, uint8 response_code) {
            if (query_packet.length < 12) {
                return query_packet;
            }

            // 计算 Question 节的精确结束位置，截掉 OPT (EDNS0) 等额外数据，避免响应报文畸形 (FORMERR)
            int idx = 12;
            while (idx < query_packet.length && query_packet[idx] != 0) {
                if ((query_packet[idx] & 0xC0) == 0xC0) {
                    idx += 2;
                    break;
                }
                idx += query_packet[idx] + 1;
            }
            if (idx < query_packet.length && query_packet[idx] == 0) {
                idx++;
            }
            idx += 4; // QTYPE (2) + QCLASS (2)
            if (idx > query_packet.length) {
                idx = query_packet.length;
            }

            uint8[] resp = new uint8[idx];
            GLib.Memory.copy (resp, query_packet, idx);

            // Response + Recursion Desired/Available，RCODE 由调用方指定。
            resp[2] = 0x81;
            resp[3] = (uint8) (0x80 | (response_code & 0x0f));

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

                // 规范跳过域名 (支持无压缩、纯指针 0xC0 或标签后接指针)
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
                    idx += len + 1;
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
                } else if (atype == 28 && rdlen == 16 && idx + 16 <= data.length) {
                    uint8[] address_bytes = new uint8[16];
                    GLib.Memory.copy (address_bytes, ((uint8*) data) + idx, 16);
                    var address = new GLib.InetAddress.from_bytes (address_bytes, GLib.SocketFamily.IPV6);
                    ips.add (address.to_string ());
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

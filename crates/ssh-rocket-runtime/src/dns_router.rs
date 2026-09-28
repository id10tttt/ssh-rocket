use anyhow::{Context, Result};
use ssh_rocket_core::{FlowContext, MatchSource, RoutingEngine, RuleAction};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use crate::system::update_domain_addresses;

pub async fn run_dns_router(
    intercept_socket: UdpSocket,
    resolver_socket: UdpSocket,
    remote_dns_port: u16,
    routing_engine: RoutingEngine,
    ipv6_enabled: bool,
    shutdown: CancellationToken,
) -> Result<()> {
    let intercept_socket = Arc::new(intercept_socket);
    let resolver_socket = Arc::new(resolver_socket);
    let routing_engine = Arc::new(routing_engine);
    let mut intercept_buffer = [0_u8; 4096];
    let mut resolver_buffer = [0_u8; 4096];
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            received = intercept_socket.recv_from(&mut intercept_buffer) => {
                let (size, peer) = received?;
                let packet = intercept_buffer[..size].to_vec();
                let socket = intercept_socket.clone();
                let routing_engine = routing_engine.clone();
                tokio::spawn(async move {
                    handle_dns_query(socket, peer, packet, remote_dns_port, routing_engine, ipv6_enabled).await;
                });
            }
            received = resolver_socket.recv_from(&mut resolver_buffer) => {
                let (size, peer) = received?;
                let packet = resolver_buffer[..size].to_vec();
                let socket = resolver_socket.clone();
                let routing_engine = routing_engine.clone();
                tokio::spawn(async move {
                    handle_dns_query(socket, peer, packet, remote_dns_port, routing_engine, ipv6_enabled).await;
                });
            }
        }
    }
}

pub async fn handle_dns_query(
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    packet: Vec<u8>,
    remote_dns_port: u16,
    routing_engine: Arc<RoutingEngine>,
    ipv6_enabled: bool,
) {
    let start_time = std::time::Instant::now();
    let Some((domain, qtype)) = parse_dns_query(&packet) else {
        return;
    };

    // 如果未开启 IPv6，对 AAAA 查询 (type 28) 立即返回 NODATA，促使客户端秒级回退至 IPv4
    if !ipv6_enabled && qtype == 28 {
        let response = nodata_response(&packet);
        let _ = socket.send_to(&response, peer).await;
        return;
    }

    let decision = routing_engine.decide(&FlowContext {
        domain: Some(domain.clone()),
        ..FlowContext::default()
    });

    if decision.action == RuleAction::Block {
        let response = nxdomain_response(&packet);
        let _ = socket.send_to(&response, peer).await;
        eprintln!("[block] {domain} (type {qtype}) -> Blocked ({:?})", decision.source);
        return;
    }

    // 对于走代理的域名，如果不是 A 记录查询（例如 AAAA type 28、HTTPS type 65、SVCB type 64 等），
    // 立即返回标准 NODATA，避免 Chrome/浏览器因收到非法的 A 记录而导致 ECH 协商失败或 IPv6 握手挂起
    if decision.action == RuleAction::Proxy && qtype != 1 {
        let response = nodata_response(&packet);
        let _ = socket.send_to(&response, peer).await;
        return;
    }

    let resolve_result = if decision.action == RuleAction::Direct {
        forward_dns_local(&packet).await
    } else {
        forward_dns_over_tcp(remote_dns_port, &packet).await
    };

    let response = match resolve_result {
        Ok(mut response) => {
            normalize_dns_response(&mut response);
            response
        }
        Err(err) => {
            eprintln!("[error] DNS query failed for {domain} (action: {:?}, source: {:?}): {err}", decision.action, decision.source);
            let servfail = servfail_response(&packet);
            let _ = socket.send_to(&servfail, peer).await;
            return;
        }
    };

    let addresses = parse_dns_addresses(&response);
    // 默认策略无需写入域名集合，否则共享 CDN 地址会覆盖显式域名规则。
    if !addresses.is_empty() && decision.source != MatchSource::DefaultPolicy {
        update_domain_addresses(&addresses, decision.action).await;
    }
    if let Err(error) = socket.send_to(&response, peer).await {
        eprintln!("[error] failed to return DNS response for {domain} to {peer}: {error}");
        return;
    }

    let elapsed = start_time.elapsed().as_millis();
    let addr_strs: Vec<String> = addresses.iter().map(|a| a.to_string()).collect();
    let addr_display = if addr_strs.is_empty() { "none".to_string() } else { addr_strs.join(", ") };

    match decision.action {
        RuleAction::Proxy => {
            eprintln!("[proxy] {domain} (type {qtype}) -> Proxy ({:?}) => [{addr_display}] ({elapsed}ms)", decision.source);
        }
        RuleAction::Direct => {
            eprintln!("[direct] {domain} (type {qtype}) -> Direct ({:?}) => [{addr_display}] ({elapsed}ms)", decision.source);
        }
        RuleAction::Block => {
            eprintln!("[block] {domain} (type {qtype}) -> Block ({:?})", decision.source);
        }
    }
}

pub async fn forward_dns_local(packet: &[u8]) -> Result<Vec<u8>> {
    match forward_dns_over_udp("223.5.5.5:53", packet, Duration::from_millis(1500)).await {
        Ok(resp) => Ok(resp),
        Err(_) => forward_dns_over_udp("114.114.114.114:53", packet, Duration::from_secs(3)).await,
    }
}

pub async fn forward_dns_over_udp(server: &str, packet: &[u8], timeout_dur: Duration) -> Result<Vec<u8>> {
    timeout(timeout_dur, async move {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.send_to(packet, server).await?;
        let mut buffer = [0_u8; 4096];
        let (size, _) = socket.recv_from(&mut buffer).await?;
        Ok::<_, anyhow::Error>(buffer[..size].to_vec())
    })
    .await
    .context("UDP DNS query timed out")?
}

pub async fn forward_dns_over_tcp(port: u16, packet: &[u8]) -> Result<Vec<u8>> {
    timeout(Duration::from_secs(6), async move {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        let length = u16::try_from(packet.len()).context("DNS packet too large")?;
        stream.write_all(&length.to_be_bytes()).await?;
        stream.write_all(packet).await?;
        let mut header = [0_u8; 2];
        stream.read_exact(&mut header).await?;
        let response_length = u16::from_be_bytes(header) as usize;
        let mut response = vec![0_u8; response_length];
        stream.read_exact(&mut response).await?;
        Ok::<_, anyhow::Error>(response)
    })
    .await
    .context("DNS forwarding over SSH timed out")?
}

pub fn parse_dns_query(packet: &[u8]) -> Option<(String, u16)> {
    if packet.len() < 12 || u16::from_be_bytes([packet[4], packet[5]]) == 0 {
        return None;
    }
    let mut offset = 12;
    let mut labels = Vec::new();
    while offset < packet.len() {
        let length = packet[offset] as usize;
        offset += 1;
        if length == 0 {
            break;
        }
        if length & 0xc0 != 0 || offset + length > packet.len() {
            return None;
        }
        labels.push(std::str::from_utf8(&packet[offset..offset + length]).ok()?);
        offset += length;
    }
    if labels.is_empty() || offset + 4 > packet.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
    Some((labels.join("."), qtype))
}

pub fn nodata_response(packet: &[u8]) -> Vec<u8> {
    empty_dns_response(packet, 0)
}

pub fn servfail_response(packet: &[u8]) -> Vec<u8> {
    empty_dns_response(packet, 2)
}

pub fn nxdomain_response(packet: &[u8]) -> Vec<u8> {
    empty_dns_response(packet, 3)
}

/// 构造只保留问题区的空 DNS 响应，避免查询中的 EDNS 记录与清零后的附加记录计数冲突。
pub fn empty_dns_response(packet: &[u8], response_code: u8) -> Vec<u8> {
    let Some(question_end) = dns_question_end(packet) else {
        return Vec::new();
    };
    let mut response = packet[..question_end].to_vec();
    response[2] = 0x80 | (packet[2] & 0x79);
    response[3] = 0x80 | (response_code & 0x0f);
    response[6..12].fill(0);
    response
}

/// 统一声明递归可用，避免 systemd-resolved 将转发响应判定为不可用服务器。
pub fn normalize_dns_response(response: &mut [u8]) {
    if response.len() < 12 {
        return;
    }
    response[2] |= 0x80;
    response[3] |= 0x80;
}

pub fn dns_question_end(packet: &[u8]) -> Option<usize> {
    if packet.len() < 12 {
        return None;
    }
    let questions = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let mut offset = 12;
    for _ in 0..questions {
        offset = skip_dns_name(packet, offset)?.checked_add(4)?;
        if offset > packet.len() {
            return None;
        }
    }
    Some(offset)
}

pub fn parse_dns_addresses(packet: &[u8]) -> Vec<IpAddr> {
    if packet.len() < 12 {
        return Vec::new();
    }
    let questions = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let answers = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let mut offset = 12;
    for _ in 0..questions {
        let Some(next) = skip_dns_name(packet, offset) else { return Vec::new(); };
        offset = next.saturating_add(4);
        if offset > packet.len() {
            return Vec::new();
        }
    }

    let mut addresses = Vec::new();
    for _ in 0..answers {
        let Some(next) = skip_dns_name(packet, offset) else { break; };
        offset = next;
        if offset + 10 > packet.len() {
            break;
        }
        let record_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let record_class = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]);
        let data_length = u16::from_be_bytes([packet[offset + 8], packet[offset + 9]]) as usize;
        offset += 10;
        if offset + data_length > packet.len() {
            break;
        }
        if record_class == 1 {
            match (record_type, data_length) {
                (1, 4) => addresses.push(IpAddr::V4(Ipv4Addr::new(
                    packet[offset], packet[offset + 1], packet[offset + 2], packet[offset + 3],
                ))),
                (28, 16) => {
                    let mut octets = [0_u8; 16];
                    octets.copy_from_slice(&packet[offset..offset + 16]);
                    addresses.push(IpAddr::V6(Ipv6Addr::from(octets)));
                }
                _ => {}
            }
        }
        offset += data_length;
    }
    addresses
}

pub fn skip_dns_name(packet: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let length = *packet.get(offset)? as usize;
        offset += 1;
        if length == 0 {
            return Some(offset);
        }
        if length & 0xc0 == 0xc0 {
            packet.get(offset)?;
            return Some(offset + 1);
        }
        offset = offset.checked_add(length)?;
        if offset > packet.len() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_with_edns(qtype: u16) -> Vec<u8> {
        let mut packet = vec![
            0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
            0x07, b'c', b'h', b'a', b't', b'g', b'p', b't',
            0x03, b'c', b'o', b'm', 0x00,
        ];
        packet.extend_from_slice(&qtype.to_be_bytes());
        packet.extend_from_slice(&1_u16.to_be_bytes());
        packet.extend_from_slice(&[0x00, 0x00, 0x29, 0x04, 0xd0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        packet
    }

    #[test]
    fn empty_response_removes_edns_record_and_keeps_question() {
        let query = query_with_edns(28);
        let response = nodata_response(&query);

        assert_eq!(&response[..2], &[0x12, 0x34]);
        assert_eq!(u16::from_be_bytes([response[4], response[5]]), 1);
        assert_eq!(&response[6..12], &[0, 0, 0, 0, 0, 0]);
        assert_eq!(response.len(), 29);
        assert_eq!(parse_dns_query(&response), Some(("chatgpt.com".into(), 28)));
    }

    #[test]
    fn empty_response_sets_expected_status_and_recursion_flags() {
        for (response, response_code) in [
            (nodata_response(&query_with_edns(65)), 0),
            (servfail_response(&query_with_edns(1)), 2),
            (nxdomain_response(&query_with_edns(1)), 3),
        ] {
            assert_ne!(response[2] & 0x80, 0);
            assert_ne!(response[2] & 0x01, 0);
            assert_ne!(response[3] & 0x80, 0);
            assert_eq!(response[3] & 0x0f, response_code);
        }
    }

    #[test]
    fn successful_response_is_marked_recursive() {
        let mut response = query_with_edns(1);
        response[2] |= 0x80;
        response[3] &= !0x80;

        normalize_dns_response(&mut response);

        assert_ne!(response[2] & 0x80, 0);
        assert_ne!(response[3] & 0x80, 0);
    }
}

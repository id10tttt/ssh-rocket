use crate::types::{ActiveConnectionDto, AppTrafficDto, ConnectionTypeDto, DesktopAppDto};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use tokio::process::Command;

pub async fn read_ssh_traffic(host: &str) -> Option<(u64, u64)> {
    if host.trim().is_empty() {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ss").args(["-tin", "dst", host]).output().await.ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let sent = sum_ss_counter(&text, "bytes_sent:");
        let received = sum_ss_counter(&text, "bytes_received:");
        (sent > 0 || received > 0).then_some((sent, received))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn sum_ss_counter(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .filter_map(|field| field.strip_prefix(key))
        .filter_map(|value| value.parse::<u64>().ok())
        .sum()
}

#[derive(Default)]
pub struct AppTrafficTracker {
    pub active_sockets: HashMap<String, (u64, u64)>,
    pub app_traffic: HashMap<String, (u64, u64)>,
    pub app_proxy_traffic: HashMap<String, (u64, u64)>,
    pub app_direct_traffic: HashMap<String, (u64, u64)>,
    pub app_local_traffic: HashMap<String, (u64, u64)>,
}

impl AppTrafficTracker {
    pub fn clear(&mut self) {
        self.active_sockets.clear();
        self.app_traffic.clear();
        self.app_proxy_traffic.clear();
        self.app_direct_traffic.clear();
        self.app_local_traffic.clear();
    }

    #[cfg(target_os = "linux")]
    pub async fn sample(
        &mut self,
        desktop_apps: &[DesktopAppDto],
        socks_port: u16,
    ) -> Option<(Vec<AppTrafficDto>, Vec<ActiveConnectionDto>)> {
        let output = Command::new("ss").args(["-tinp", "-H"]).output().await.ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut seen_sockets = HashSet::new();
        let mut current_sock_key = String::new();
        let mut current_proc_name = String::new();
        let mut current_local_addr = String::new();
        let mut current_peer_addr = String::new();

        let mut raw_conns: Vec<(String, String, String, u64, u64, ConnectionTypeDto)> = Vec::new();

        for line in text.lines() {
            if !line.starts_with([' ', '\t']) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let local = parts[3];
                    let peer = parts[4];
                    current_local_addr = local.to_string();
                    current_peer_addr = peer.to_string();
                    current_sock_key = format!("{local}->{peer}");
                    seen_sockets.insert(current_sock_key.clone());

                    current_proc_name = if let Some(idx) = line.find("users:((\"") {
                        let start = idx + 9;
                        if let Some(end) = line[start..].find('"') {
                            line[start..start + end].to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                } else {
                    current_sock_key.clear();
                    current_proc_name.clear();
                    current_local_addr.clear();
                    current_peer_addr.clear();
                }
            } else if !current_sock_key.is_empty() && !current_proc_name.is_empty() {
                let cur_sent = sum_ss_counter(line, "bytes_sent:");
                let cur_received = sum_ss_counter(line, "bytes_received:");
                if cur_sent > 0 || cur_received > 0 {
                    let (delta_up, delta_down) = if let Some((prev_sent, prev_rcv)) =
                        self.active_sockets.get(&current_sock_key)
                    {
                        (
                            cur_sent.saturating_sub(*prev_sent),
                            cur_received.saturating_sub(*prev_rcv),
                        )
                    } else {
                        (cur_sent, cur_received)
                    };
                    self.active_sockets
                        .insert(current_sock_key.clone(), (cur_sent, cur_received));

                    let is_socks = current_peer_addr.ends_with(&format!(":{socks_port}"))
                        || (current_local_addr.ends_with(&format!(":{socks_port}"))
                            && current_proc_name == "ssh");
                    let is_loopback = current_peer_addr.starts_with("127.")
                        || current_peer_addr.starts_with("[::1]")
                        || current_peer_addr.starts_with("::1");

                    let conn_type = if is_socks {
                        ConnectionTypeDto::Proxy
                    } else if is_loopback {
                        ConnectionTypeDto::Local
                    } else {
                        ConnectionTypeDto::Direct
                    };

                    if delta_up > 0 || delta_down > 0 {
                        let entry = self
                            .app_traffic
                            .entry(current_proc_name.clone())
                            .or_insert((0, 0));
                        entry.0 += delta_up;
                        entry.1 += delta_down;

                        match conn_type {
                            ConnectionTypeDto::Proxy => {
                                let p_entry = self
                                    .app_proxy_traffic
                                    .entry(current_proc_name.clone())
                                    .or_insert((0, 0));
                                p_entry.0 += delta_up;
                                p_entry.1 += delta_down;
                            }
                            ConnectionTypeDto::Direct => {
                                let d_entry = self
                                    .app_direct_traffic
                                    .entry(current_proc_name.clone())
                                    .or_insert((0, 0));
                                d_entry.0 += delta_up;
                                d_entry.1 += delta_down;
                            }
                            ConnectionTypeDto::Local => {
                                let l_entry = self
                                    .app_local_traffic
                                    .entry(current_proc_name.clone())
                                    .or_insert((0, 0));
                                l_entry.0 += delta_up;
                                l_entry.1 += delta_down;
                            }
                        }
                    }

                    raw_conns.push((
                        current_proc_name.clone(),
                        current_local_addr.clone(),
                        current_peer_addr.clone(),
                        cur_sent,
                        cur_received,
                        conn_type,
                    ));
                }
            }
        }

        self.active_sockets.retain(|k, _| seen_sockets.contains(k));

        let mut stats: Vec<AppTrafficDto> = self
            .app_traffic
            .iter()
            .map(|(proc, (up, down))| {
                let matching_app = desktop_apps.iter().find(|app| {
                    app.executable.eq_ignore_ascii_case(proc)
                        || app.name.eq_ignore_ascii_case(proc)
                        || Path::new(&app.executable)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(proc))
                });
                let (name, icon) = if let Some(app) = matching_app {
                    (app.name.clone(), app.icon.clone())
                } else {
                    (proc.clone(), String::new())
                };
                let (p_up, p_down) = self.app_proxy_traffic.get(proc).copied().unwrap_or((0, 0));
                let (d_up, d_down) = self.app_direct_traffic.get(proc).copied().unwrap_or((0, 0));
                let (l_up, l_down) = self.app_local_traffic.get(proc).copied().unwrap_or((0, 0));

                let proxy_total = p_up + p_down;
                let direct_total = d_up + d_down;
                let local_total = l_up + l_down;

                let primary_type = if proxy_total >= direct_total && proxy_total >= local_total && proxy_total > 0 {
                    ConnectionTypeDto::Proxy
                } else if direct_total >= local_total && direct_total > 0 {
                    ConnectionTypeDto::Direct
                } else {
                    ConnectionTypeDto::Local
                };

                AppTrafficDto {
                    id: proc.clone(),
                    name,
                    icon,
                    upload: *up,
                    download: *down,
                    proxy_upload: p_up,
                    proxy_download: p_down,
                    direct_upload: d_up,
                    direct_download: d_down,
                    local_upload: l_up,
                    local_download: l_down,
                    primary_type,
                }
            })
            .collect();

        stats.sort_by(|a, b| (b.upload + b.download).cmp(&(a.upload + a.download)));

        raw_conns.sort_by(|a, b| (b.3 + b.4).cmp(&(a.3 + a.4)));
        let active_conns: Vec<ActiveConnectionDto> = raw_conns
            .into_iter()
            .take(60)
            .map(|(proc, local, peer, up, down, conn_type)| {
                let matching_app = desktop_apps.iter().find(|app| {
                    app.executable.eq_ignore_ascii_case(&proc)
                        || app.name.eq_ignore_ascii_case(&proc)
                        || Path::new(&app.executable)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(&proc))
                });
                let (name, icon) = if let Some(app) = matching_app {
                    (app.name.clone(), app.icon.clone())
                } else {
                    (proc, String::new())
                };
                ActiveConnectionDto {
                    proc_name: name,
                    icon,
                    local_addr: local,
                    peer_addr: peer,
                    conn_type,
                    upload: up,
                    download: down,
                }
            })
            .collect();

        Some((stats, active_conns))
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn sample(
        &mut self,
        _desktop_apps: &[DesktopAppDto],
        _socks_port: u16,
    ) -> Option<(Vec<AppTrafficDto>, Vec<ActiveConnectionDto>)> {
        None
    }
}

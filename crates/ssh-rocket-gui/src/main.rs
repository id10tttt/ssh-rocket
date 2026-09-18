mod tray;
pub mod ui;

use adw::prelude::*;
use gtk4::{self as gtk, gio};
use libadwaita as adw;
use ssh_rocket_core::{
    parse_omega_rules, parse_rule_set, parse_shadowrocket_rules, AppConfig, AppRule, DomainRule,
    DomainRuleKind, IpRule, Profile, RuleAction, RuleImportResult,
};
use ssh_rocket_runtime::{PrivilegedHelperSession, SshSession};
use std::{
    cell::RefCell,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{oneshot, Mutex},
};
use tray::{TrayConnectionState, TrayManager};
use ui::{
    connect_view::{render_connection_cards, ConnectView},
    dialogs::{show_profile_dialog, show_rule_dialog, RefreshConnections, RefreshRules},
    logs_view::LogsView,
    rules_view::{append_rule_batch, refresh_rule_list, RulesView},
    theme::init_theme,
    traffic_view::{refresh_app_traffic_list, refresh_traffic_rule_counts, TrafficView},
    widgets::{create_app_icon, format_bytes, format_duration, format_speed},
    window::create_main_window,
};

pub const APP_ID: &str = "io.github.idi0t.SshRocket";
pub const SOCKS_PORT: u16 = 17880;
pub const DEFAULT_RULE_SOURCE: &str =
    "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
pub const MAX_RULE_SOURCE_SIZE: usize = 16 * 1024 * 1024;
pub const RULE_BATCH_SIZE: usize = 20;

#[derive(Clone, Debug, Default)]
pub struct AppTrafficStat {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub upload: u64,
    pub download: u64,
}

pub enum RuntimeEvent {
    Connected,
    Disconnected,
    Status(String),
    Error(String),
    Log(String),
    Speed { upload: u64, download: u64 },
    AppTraffic(Vec<AppTrafficStat>),
    RuleImportFailed(String),
    RulesImported {
        result: RuleImportResult,
        source_url: String,
    },
}

#[derive(Clone, Default)]
pub struct RuntimeController {
    stop: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    helper: Arc<Mutex<Option<PrivilegedHelperSession>>>,
    is_running: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct DesktopApp {
    pub name: String,
    pub executable: String,
    pub icon: String,
}

#[derive(Clone)]
pub enum ListedRule {
    Domain(DomainRule),
    Ip(IpRule),
}

impl ListedRule {
    pub fn value(&self) -> String {
        match self {
            Self::Domain(rule) => rule.pattern.clone(),
            Self::Ip(rule) => rule.network.to_string(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Domain(rule) => domain_kind_label(rule.kind),
            Self::Ip(_) => "IP-CIDR",
        }
    }

    pub fn action(&self) -> RuleAction {
        match self {
            Self::Domain(rule) => rule.action,
            Self::Ip(rule) => rule.action,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.value().to_lowercase().contains(query)
            || self.kind_label().to_lowercase().contains(query)
            || action_label(self.action()).to_lowercase().contains(query)
    }
}

#[derive(Default)]
pub struct RuleListState {
    pub filtered: Vec<ListedRule>,
    pub rendered_rows: Vec<adw::ActionRow>,
    pub loaded: usize,
}

pub fn action_label(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Direct => "DIRECT",
        RuleAction::Proxy => "PROXY",
        RuleAction::Block => "REJECT",
    }
}

pub fn domain_kind_label(kind: DomainRuleKind) -> &'static str {
    match kind {
        DomainRuleKind::Domain => "DOMAIN",
        DomainRuleKind::DomainSuffix => "DOMAIN-SUFFIX",
        DomainRuleKind::DomainKeyword => "DOMAIN-KEYWORD",
        DomainRuleKind::Legacy => "LEGACY",
    }
}

pub fn custom_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(config.settings.ip_rules.iter().cloned().map(ListedRule::Ip))
        .collect()
}

pub fn imported_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .imported_domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(
            config
                .settings
                .imported_ip_rules
                .iter()
                .cloned()
                .map(ListedRule::Ip),
        )
        .collect()
}

pub fn rule_source_name(source_url: &str) -> String {
    source_url
        .split(['?', '#'])
        .next()
        .and_then(|url| url.rsplit('/').find(|part| !part.is_empty()))
        .filter(|name| !name.is_empty())
        .unwrap_or("订阅规则")
        .to_string()
}

pub fn remove_listed_rule(config: &mut AppConfig, rule: &ListedRule) {
    match rule {
        ListedRule::Domain(rule) => config
            .settings
            .domain_rules
            .retain(|item| !(item.pattern == rule.pattern && item.kind == rule.kind)),
        ListedRule::Ip(rule) => config
            .settings
            .ip_rules
            .retain(|item| item.network != rule.network),
    }
}

pub fn scan_desktop_apps() -> Vec<DesktopApp> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }

    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse_desktop_app(&text) {
                if seen.insert(app.executable.clone()) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn parse_desktop_app(text: &str) -> Option<DesktopApp> {
    let mut in_entry = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut no_display = false;
    let mut app_type = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            if name.is_empty() {
                name = value.trim().to_string();
            }
        } else if let Some(value) = line.strip_prefix("Exec=") {
            exec = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Icon=") {
            icon = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            no_display = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Type=") {
            app_type = value.trim().to_string();
        }
    }
    if no_display || (!app_type.is_empty() && app_type != "Application") || exec.is_empty() {
        return None;
    }
    let executable = extract_exec_name(&exec)?;
    Some(DesktopApp {
        name: if name.is_empty() {
            executable.clone()
        } else {
            name
        },
        executable,
        icon,
    })
}

fn extract_exec_name(exec: &str) -> Option<String> {
    let mut parts = exec
        .split_whitespace()
        .filter(|part| !part.starts_with('%'));
    let first = parts.next()?.trim_matches(['\'', '"']);
    let command = if first.ends_with("/env") || first == "env" {
        parts.find(|part| !part.starts_with('-') && !part.contains('='))?
    } else {
        first
    };
    if command.ends_with("flatpak") || command == "flatpak" {
        let args: Vec<_> = exec.split_whitespace().collect();
        if let Some(value) = args.iter().find_map(|arg| arg.strip_prefix("--command=")) {
            return Path::new(value)
                .file_name()
                .map(|value| value.to_string_lossy().to_lowercase());
        }
        if let Some(id) = args
            .iter()
            .rev()
            .find(|arg| !arg.starts_with('-') && **arg != "run")
        {
            return id
                .rsplit('.')
                .find(|part| !matches!(*part, "desktop" | "client" | "app"))
                .map(|value| value.to_lowercase());
        }
    }
    Path::new(command)
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
}

pub fn current_app_action(config: &AppConfig, executable: &str) -> RuleAction {
    config
        .settings
        .app_rules
        .iter()
        .find(|rule| {
            rule.executable
                .file_name()
                .is_some_and(|name| name == executable)
        })
        .map(|rule| rule.action)
        .unwrap_or(RuleAction::Direct)
}

pub fn set_app_action(config: &Rc<RefCell<AppConfig>>, executable: &str, action: RuleAction) {
    let mut current = config.borrow_mut();
    current.settings.app_rules.retain(|rule| {
        rule.executable
            .file_name()
            .is_none_or(|name| name != executable)
    });
    current.settings.app_rules.push(AppRule {
        executable: PathBuf::from(executable),
        action,
    });
    let _ = current.save();
}

impl RuntimeController {
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(stop) = self.stop.borrow_mut().take() {
            let _ = stop.send(());
        }
    }

    pub fn shutdown(&self) {
        self.stop();
        let helper = self.helper.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = runtime {
                rt.block_on(async {
                    let mut guard = helper.lock().await;
                    if let Some(mut h) = guard.take() {
                        h.shutdown().await;
                    }
                });
            }
        });
    }

    pub fn sync_rules(&self) {
        let helper = self.helper.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = runtime {
                rt.block_on(async {
                    let mut guard = helper.lock().await;
                    if let Some(h) = guard.as_mut() {
                        let _ = h.sync_rules().await;
                    }
                });
            }
        });
    }

    pub fn start(
        &self,
        profile: Profile,
        config: AppConfig,
        config_path: PathBuf,
        events: mpsc::Sender<RuntimeEvent>,
    ) {
        self.stop();
        self.is_running.store(true, Ordering::SeqCst);
        let (stop_tx, mut stop_rx) = oneshot::channel();
        *self.stop.borrow_mut() = Some(stop_tx);

        let helper = self.helper.clone();
        let is_running_flag = self.is_running.clone();

        thread::spawn(move || {
            let desktop_apps = scan_desktop_apps();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();
            let Ok(runtime) = runtime else {
                let _ = events.send(RuntimeEvent::Error("无法创建异步运行时".into()));
                is_running_flag.store(false, Ordering::SeqCst);
                return;
            };
            runtime.block_on(async move {
                let user_cancelled = Arc::new(AtomicBool::new(false));
                let mut retry_attempt = 0;
                let mut app_tracker = AppTrafficTracker::default();

                loop {
                    if user_cancelled.load(Ordering::SeqCst) {
                        break;
                    }

                    if retry_attempt > 0 {
                        let _ = events.send(RuntimeEvent::Status(format!("正在重连 ({retry_attempt})…")));
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[reconnect] 正在重新建立 SSH 连接 (第 {retry_attempt} 次)..."
                        )));
                    }

                    // 1. 验证 helper 可响应，并在建立 SSH 前清理遗留的特权网络状态。
                    let mut helper_guard = helper.lock().await;
                    let helper_usable = match helper_guard.as_mut() {
                        Some(h) => {
                            if !h.is_alive() {
                                false
                            } else {
                                match h.status().await {
                                    Ok(false) => true,
                                    Ok(true) => match h.stop().await {
                                        Ok(()) => true,
                                        Err(error) => {
                                            let _ = events.send(RuntimeEvent::Log(format!(
                                                "[helper] Failed to reset active helper session: {error}"
                                            )));
                                            false
                                        }
                                    },
                                    Err(error) => {
                                        let _ = events.send(RuntimeEvent::Log(format!(
                                            "[helper] Existing helper is unresponsive: {error}"
                                        )));
                                        false
                                    }
                                }
                            }
                        }
                        None => false,
                    };
                    if !helper_usable {
                        if let Some(mut stale_helper) = helper_guard.take() {
                            stale_helper.terminate().await;
                        }
                        let _ = events.send(RuntimeEvent::Log(
                            "[helper] 请求特权 Helper 授权...".into(),
                        ));
                        let _ = events.send(RuntimeEvent::Status("正在授权 Helper…".into()));
                        match PrivilegedHelperSession::ensure_started(&helper_path()).await {
                            Ok((h, stderr)) => {
                                if let Some(stderr) = stderr {
                                    spawn_log_reader(stderr, "helper", events.clone());
                                }
                                *helper_guard = Some(h);
                                let _ = events.send(RuntimeEvent::Log(
                                    "[helper] 特权 Helper 认证成功并就绪".into(),
                                ));
                            }
                            Err(err) => {
                                if user_cancelled.load(Ordering::SeqCst) {
                                    break;
                                }
                                let _ = events.send(RuntimeEvent::Error(format!(
                                    "Helper 授权失败: {err}"
                                )));
                                let _ = events.send(RuntimeEvent::Log(format!(
                                    "[helper] 授权失败: {err}"
                                )));
                                break;
                            }
                        }
                    }

                    // 2. 建立 OpenSSH 会话
                    let mut ssh = match SshSession::start(
                        &profile,
                        SOCKS_PORT,
                        config.settings.dns_server,
                    )
                    .await
                    {
                        Ok(session) => session,
                        Err(error) => {
                            drop(helper_guard);
                            if user_cancelled.load(Ordering::SeqCst) {
                                break;
                            }
                            retry_attempt += 1;
                            let _ = events.send(RuntimeEvent::Log(format!(
                                "[reconnect] SSH 连接失败: {error}。1秒后重试 (第 {retry_attempt} 次)..."
                            )));
                            let _ = events.send(RuntimeEvent::Status("1秒后重试…".into()));
                            tokio::select! {
                                _ = &mut stop_rx => {
                                    user_cancelled.store(true, Ordering::SeqCst);
                                    break;
                                }
                                _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                            }
                        }
                    };

                    if let Some(stderr) = ssh.take_stderr() {
                        spawn_log_reader(stderr, "ssh", events.clone());
                    }

                    let uid = unsafe { libc::getuid() };
                    let helper_session = helper_guard.as_mut().unwrap();
                    let start_res = helper_session
                        .start(
                            config_path.clone(),
                            uid,
                            ssh.socks_port,
                            ssh.dns_port,
                            ssh.server_port,
                            ssh.server_addresses.clone(),
                        )
                        .await;

                    if let Err(err) = start_res {
                        let _ = ssh.stop().await;
                        if let Some(mut failed_helper) = helper_guard.take() {
                            failed_helper.terminate().await;
                        }
                        if user_cancelled.load(Ordering::SeqCst) {
                            break;
                        }
                        retry_attempt += 1;
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[reconnect] 透明代理启动失败: {err}。1秒后重试 (第 {retry_attempt} 次)..."
                        )));
                        let _ = events.send(RuntimeEvent::Status("1秒后重试…".into()));
                        drop(helper_guard);
                        tokio::select! {
                            _ = &mut stop_rx => {
                                user_cancelled.store(true, Ordering::SeqCst);
                                break;
                            }
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    }

                    // 连接成功
                    if retry_attempt > 0 {
                        let _ = events.send(RuntimeEvent::Log("[reconnect] 连接已恢复".into()));
                    }
                    retry_attempt = 0;
                    let _ = events.send(RuntimeEvent::Connected);

                    // 3. 监听活动连接
                    let mut traffic_interval = tokio::time::interval(Duration::from_secs(1));
                    let mut previous_traffic = None;
                    let mut disconnect_reason = String::new();

                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => {
                                user_cancelled.store(true, Ordering::SeqCst);
                                break;
                            }
                            status = ssh.wait() => {
                                disconnect_reason = match status {
                                    Ok(status) => format!("SSH 进程退出，状态码: {status}"),
                                    Err(error) => error.to_string(),
                                };
                                let _ = events.send(RuntimeEvent::Log(format!("[reconnect] {disconnect_reason}")));
                                break;
                            }
                            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                                let helper_health = match helper_guard.as_mut() {
                                    Some(helper) => helper.check_active().await,
                                    None => Err(anyhow::anyhow!("特权 Helper 会话不可用")),
                                };
                                if let Err(error) = helper_health {
                                    disconnect_reason = format!("透明代理健康检查失败: {error}");
                                    let _ = events.send(RuntimeEvent::Log(format!("[helper] {disconnect_reason}")));
                                    break;
                                }
                            }
                            _ = traffic_interval.tick() => {
                                if let Some((sent, received)) = read_ssh_traffic(&profile.host).await {
                                    let (upload, download) = previous_traffic
                                        .map(|(old_sent, old_received)| {
                                            (
                                                sent.saturating_sub(old_sent),
                                                received.saturating_sub(old_received),
                                            )
                                        })
                                        .unwrap_or((0, 0));
                                    previous_traffic = Some((sent, received));
                                    let _ = events.send(RuntimeEvent::Speed { upload, download });
                                }
                                if let Some(stats) = app_tracker.sample(&desktop_apps).await {
                                    let _ = events.send(RuntimeEvent::AppTraffic(stats));
                                }
                            }
                        }
                    }

                    let helper_stop_error = match helper_guard.as_mut() {
                        Some(h) => h.stop().await.err(),
                        None => None,
                    };
                    if let Some(error) = helper_stop_error {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[helper] 停止 helper 失败: {error}"
                        )));
                        if let Some(mut failed_helper) = helper_guard.take() {
                            failed_helper.terminate().await;
                        }
                    }
                    drop(helper_guard);
                    let _ = ssh.stop().await;
                    let _ = events.send(RuntimeEvent::Speed {
                        upload: 0,
                        download: 0,
                    });
                    app_tracker.clear();

                    if user_cancelled.load(Ordering::SeqCst) {
                        let _ = events.send(RuntimeEvent::Disconnected);
                        break;
                    }

                    retry_attempt += 1;
                    let _ = events.send(RuntimeEvent::Status("连接断开，正在准备重连…".into()));
                    let _ = events.send(RuntimeEvent::Log(format!(
                        "[reconnect] 连接已断开 ({disconnect_reason})。1秒后尝试重连 (第 {retry_attempt} 次)..."
                    )));
                    tokio::select! {
                        _ = &mut stop_rx => {
                            user_cancelled.store(true, Ordering::SeqCst);
                            let _ = events.send(RuntimeEvent::Disconnected);
                            break;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                    }
                }

                is_running_flag.store(false, Ordering::SeqCst);
            });
        });
    }
}

async fn read_ssh_traffic(host: &str) -> Option<(u64, u64)> {
    if host.trim().is_empty() {
        return None;
    }
    let output = Command::new("ss").args(["-tin", "dst", host]).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let sent = sum_ss_counter(&text, "bytes_sent:");
    let received = sum_ss_counter(&text, "bytes_received:");
    (sent > 0 || received > 0).then_some((sent, received))
}

fn sum_ss_counter(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .filter_map(|field| field.strip_prefix(key))
        .filter_map(|value| value.parse::<u64>().ok())
        .sum()
}

#[derive(Default)]
struct AppTrafficTracker {
    active_sockets: std::collections::HashMap<String, (u64, u64)>,
    app_traffic: std::collections::HashMap<String, (u64, u64)>,
}

impl AppTrafficTracker {
    fn clear(&mut self) {
        self.active_sockets.clear();
        self.app_traffic.clear();
    }

    async fn sample(&mut self, desktop_apps: &[DesktopApp]) -> Option<Vec<AppTrafficStat>> {
        let output = Command::new("ss").args(["-tinp", "-H"]).output().await.ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut seen_sockets = std::collections::HashSet::new();
        let mut current_sock_key = String::new();
        let mut current_proc_name = String::new();

        for line in text.lines() {
            if !line.starts_with([' ', '\t']) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let local = parts[3];
                    let peer = parts[4];
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
                    if delta_up > 0 || delta_down > 0 {
                        let entry = self
                            .app_traffic
                            .entry(current_proc_name.clone())
                            .or_insert((0, 0));
                        entry.0 += delta_up;
                        entry.1 += delta_down;
                    }
                }
            }
        }

        self.active_sockets.retain(|k, _| seen_sockets.contains(k));

        let mut stats: Vec<AppTrafficStat> = self
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
                AppTrafficStat {
                    id: proc.clone(),
                    name,
                    icon,
                    upload: *up,
                    download: *down,
                }
            })
            .collect();

        stats.sort_by(|a, b| (b.upload + b.download).cmp(&(a.upload + a.download)));
        Some(stats)
    }
}

fn spawn_log_reader<R>(reader: R, source: &'static str, events: mpsc::Sender<RuntimeEvent>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = events.send(RuntimeEvent::Log(format!("[{source}] {line}")));
        }
    });
}

fn import_rule_source(url: &str) -> Result<RuleImportResult, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let content = download_rule_text(url)?;
    let mut result = parse_shadowrocket_rules(&content);
    let rule_sets = result.rule_sets.clone();
    for reference in rule_sets.into_iter().take(8) {
        match download_rule_text(&reference.url) {
            Ok(content) => result.merge(parse_rule_set(&content, reference.action)),
            Err(error) => {
                result.ignored_count += 1;
                result.warnings.push(format!("子规则集跳过: {error}"));
            }
        }
    }
    if result.rule_sets.len() > 8 {
        result.ignored_count += result.rule_sets.len() - 8;
        result.warnings.push("部分超出数量限制的子规则集已被跳过".into());
    }
    if result.rule_count() == 0 {
        return Err("规则源不包含任何支持的有效规则".into());
    }
    Ok(result)
}

fn download_rule_text(url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let output = StdCommand::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "120",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            &MAX_RULE_SOURCE_SIZE.to_string(),
            url,
        ])
        .output()
        .map_err(|error| format!("curl 命令启动失败: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            format!("curl 进程退出码: {}", output.status)
        } else {
            error
        });
    }
    if output.stdout.len() > MAX_RULE_SOURCE_SIZE {
        return Err("规则源文件大小超过 16 MB 限制".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "规则内容非有效 UTF-8 编码".into())
}

fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SSH_ROCKET_HELPER") {
        return PathBuf::from(path);
    }
    for path in [
        "/usr/local/libexec/ssh-rocket-helper",
        "/usr/libexec/ssh-rocket-helper",
    ] {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("ssh-rocket-helper")))
        .unwrap_or_else(|| PathBuf::from("ssh-rocket-helper"))
}

fn build_ui(app: &adw::Application) {
    init_theme();

    let config = Rc::new(RefCell::new(AppConfig::load().unwrap_or_default()));
    let controller = Rc::new(RefCell::new(RuntimeController::default()));
    let (event_tx, event_rx) = mpsc::channel::<RuntimeEvent>();
    let is_connected = Rc::new(RefCell::new(false));
    let connect_start_time = Rc::new(RefCell::new(None::<std::time::Instant>));
    let connection_buttons = Rc::new(RefCell::new(Vec::<(String, gtk::Button)>::new()));
    let refresh_connections: RefreshConnections = Rc::new(RefCell::new(None));
    let tray_manager = Rc::new(RefCell::new(None::<Rc<TrayManager>>));
    let quitting = Rc::new(RefCell::new(false));

    // 1. 构建主窗口与导航
    let win = create_main_window(app);

    // 2. 构建各功能视图
    let connect_view = ConnectView::new();
    let rules_view = RulesView::new(&config);
    let traffic_view = TrafficView::new();
    let logs_view = LogsView::new();

    win.view_stack
        .add_named(&connect_view.container, Some("connect"));
    win.view_stack.add_named(&rules_view.container, Some("rules"));
    win.view_stack.add_named(&traffic_view.page, Some("traffic"));
    win.view_stack.add_named(&logs_view.container, Some("logs"));

    // 3. 侧边栏导航切换
    {
        let view_stack = win.view_stack.clone();
        let page_title = win.page_title.clone();
        let add_connection = win.add_connection.clone();
        win.navigation.connect_row_selected(move |_, row| {
            let Some(row) = row else { return; };
            let (name, title) = match row.index() {
                1 => ("rules", "分流规则"),
                2 => ("traffic", "流量监控"),
                3 => ("logs", "运行日志"),
                _ => ("connect", "节点连接"),
            };
            view_stack.set_visible_child_name(name);
            page_title.set_text(title);
            add_connection.set_visible(name == "connect");
        });
    }
    win.navigation.select_row(Some(&win.connect_nav));

    // 4. 节点列表渲染与刷新
    let refresh_rule_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_blocked_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_traffic_rule_counts_fn: Rc<RefCell<Option<Rc<dyn Fn()>>>> =
        Rc::new(RefCell::new(None));

    {
        let connection_flow = connect_view.connection_flow.clone();
        let connect_stack = connect_view.container.clone();
        let config = config.clone();
        let controller = controller.clone();
        let event_tx = event_tx.clone();
        let is_connected = is_connected.clone();
        let connection_buttons = connection_buttons.clone();
        let refresh_handle = refresh_connections.clone();
        let tray_manager = tray_manager.clone();
        let parent = win.window.clone();
        let bottom_status = win.bottom_status.clone();
        let refresh_impl: Rc<dyn Fn()> = Rc::new(move || {
            render_connection_cards(
                &connection_flow,
                &connect_stack,
                &config,
                &controller,
                &event_tx,
                &is_connected,
                &connection_buttons,
                &refresh_handle,
                &tray_manager,
                &parent,
                &bottom_status,
            );
        });
        *refresh_connections.borrow_mut() = Some(refresh_impl.clone());
        refresh_impl();
    }

    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        win.add_connection.connect_clicked(move |_| {
            show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        connect_view.empty_add_button.connect_clicked(move |_| {
            show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
        });
    }

    // 5. 应用列表初始化
    for app_info in scan_desktop_apps() {
        let action = current_app_action(&config.borrow(), &app_info.executable);
        let row = adw::ComboRow::builder()
            .title(&app_info.name)
            .subtitle(&app_info.executable)
            .model(&gtk::StringList::new(&["直连 (Direct)", "代理 (Proxy)", "拦截 (Block)"]))
            .selected(match action {
                RuleAction::Direct => 0,
                RuleAction::Proxy => 1,
                RuleAction::Block => 2,
            })
            .build();
        row.set_use_markup(false);
        row.add_prefix(&create_app_icon(&app_info.icon));
        let executable = app_info.executable.clone();
        let config_ref = config.clone();
        let controller_ref = controller.clone();
        let refresh_blocked_ref = refresh_blocked_views.clone();
        let refresh_traffic_counts_ref = refresh_traffic_rule_counts_fn.clone();
        row.connect_selected_notify(move |row| {
            let action = match row.selected() {
                1 => RuleAction::Proxy,
                2 => RuleAction::Block,
                _ => RuleAction::Direct,
            };
            set_app_action(&config_ref, &executable, action);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_counts_ref.borrow().as_ref() {
                refresh();
            }
        });
        rules_view.applications_group.add(&row);
        rules_view.app_rows.borrow_mut().push((
            format!("{} {}", app_info.name, app_info.executable).to_lowercase(),
            app_info.name.to_lowercase(),
            row,
        ));
    }
    {
        let app_rows = rules_view.app_rows.clone();
        rules_view.app_search.connect_search_changed(move |entry| {
            let query = entry.text().to_lowercase();
            for (search_text, _, row) in app_rows.borrow().iter() {
                row.set_visible(query.is_empty() || search_text.contains(&query));
            }
        });
    }
    {
        let app_rows = rules_view.app_rows.clone();
        let applications_group = rules_view.applications_group.clone();
        rules_view.app_sort.connect_selected_notify(move |sort| {
            let mut rows = app_rows.borrow_mut();
            rows.sort_by(|left, right| {
                if sort.selected() == 1 {
                    left.2
                        .selected()
                        .cmp(&right.2.selected())
                        .then_with(|| left.1.cmp(&right.1))
                } else {
                    left.1.cmp(&right.1)
                }
            });
            for (_, _, row) in rows.iter() {
                applications_group.remove(row);
            }
            for (_, _, row) in rows.iter() {
                applications_group.add(row);
            }
        });
    }

    // 6. 域名与 IP 规则逻辑
    {
        let config = config.clone();
        rules_view.policy_row.connect_selected_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.default_policy = match row.selected() {
                1 => RuleAction::Direct,
                2 => RuleAction::Block,
                _ => RuleAction::Proxy,
            };
            let _ = current.save();
        });
    }
    {
        let config = config.clone();
        rules_view.ipv6_row.connect_active_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.ipv6 = row.is_active();
            let _ = current.save();
        });
    }

    let imported_state = Rc::new(RefCell::new(RuleListState::default()));
    let custom_state = Rc::new(RefCell::new(RuleListState::default()));

    {
        let group = rules_view.imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = rules_view.imported_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.imported_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
        });
    }
    {
        let group = rules_view.custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = rules_view.custom_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.custom_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
        });
    }
    {
        let group = rules_view.imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = rules_view.imported_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view
            .imported_scroller
            .vadjustment()
            .connect_value_changed(move |adjustment| {
                if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                    append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
                }
            });
    }
    {
        let group = rules_view.custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = rules_view.custom_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view
            .custom_scroller
            .vadjustment()
            .connect_value_changed(move |adjustment| {
                if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                    append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
                }
            });
    }

    let refresh_rule_views_impl: Rc<dyn Fn()> = {
        let config = config.clone();
        let rule_status = rules_view.rule_status_row.clone();
        let custom_summary = rules_view.custom_summary_row.clone();
        let detail_title = rules_view.detail_title_lbl.clone();
        let source_detail = rules_view.source_detail_row.clone();
        let imported_summary = rules_view.imported_summary_row.clone();
        let imported_search = rules_view.imported_search.clone();
        let imported_rules_group = rules_view.imported_rules_group.clone();
        let imported_state = imported_state.clone();
        let imported_load_more = rules_view.imported_load_more.clone();
        let custom_search = rules_view.custom_search.clone();
        let custom_rules_group = rules_view.custom_rules_group.clone();
        let custom_state = custom_state.clone();
        let custom_load_more = rules_view.custom_load_more.clone();
        let clear_rules = rules_view.clear_rules_btn.clone();
        let parent = win.window.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        let refresh_blocked_views = refresh_blocked_views.clone();
        let refresh_traffic_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let current = config.borrow();
            let imported = imported_rules(&current);
            let custom = custom_rules(&current);
            let source_name = if current.settings.rule_source_name.is_empty() {
                "订阅规则"
            } else {
                &current.settings.rule_source_name
            };
            if imported.is_empty() {
                rule_status.set_title("未配置远程规则");
                rule_status.set_subtitle("");
                rule_status.set_activatable(false);
            } else {
                rule_status.set_title(source_name);
                rule_status.set_subtitle(&format!("共 {} 条规则", imported.len()));
                rule_status.set_activatable(true);
            }
            custom_summary.set_subtitle(&format!("共 {} 条规则", custom.len()));
            detail_title.set_text(source_name);
            source_detail.set_title(source_name);
            source_detail.set_subtitle(&current.settings.rule_source_url);
            let (direct, proxy, reject) = imported.iter().fold((0, 0, 0), |counts, rule| {
                match rule.action() {
                    RuleAction::Direct => (counts.0 + 1, counts.1, counts.2),
                    RuleAction::Proxy => (counts.0, counts.1 + 1, counts.2),
                    RuleAction::Block => (counts.0, counts.1, counts.2 + 1),
                }
            });
            imported_summary.set_subtitle(&format!(
                "共 {} 条 · 直连 {direct} · 代理 {proxy} · 拦截 {reject}",
                imported.len()
            ));
            clear_rules.set_visible(!custom.is_empty());
            let imported_query = imported_search.text().to_string();
            let custom_query = custom_search.text().to_string();
            drop(current);
            refresh_rule_list(
                &imported_rules_group,
                &imported_state,
                &imported_load_more,
                imported,
                &imported_query,
                false,
                &parent,
                &config,
                &refresh_rule_views,
            );
            refresh_rule_list(
                &custom_rules_group,
                &custom_state,
                &custom_load_more,
                custom,
                &custom_query,
                true,
                &parent,
                &config,
                &refresh_rule_views,
            );
            if let Some(refresh) = refresh_blocked_views.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    *refresh_rule_views.borrow_mut() = Some(refresh_rule_views_impl.clone());
    refresh_rule_views_impl();

    {
        let refresh = refresh_rule_views_impl.clone();
        rules_view
            .imported_search
            .connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_rule_views_impl.clone();
        rules_view
            .custom_search
            .connect_search_changed(move |_| refresh());
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .rule_status_row
            .connect_activated(move |_| stack.set_visible_child_name("detail"));
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .custom_summary_row
            .connect_activated(move |_| stack.set_visible_child_name("custom"));
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .detail_back_btn
            .connect_clicked(move |_| stack.set_visible_child_name("overview"));
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .imported_summary_row
            .connect_activated(move |_| stack.set_visible_child_name("imported"));
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .imported_back_btn
            .connect_clicked(move |_| stack.set_visible_child_name("detail"));
    }
    {
        let stack = rules_view.domain_stack.clone();
        rules_view
            .custom_back_btn
            .connect_clicked(move |_| stack.set_visible_child_name("overview"));
    }
    {
        let parent = win.window.clone();
        let trigger = rules_view.import_trigger_btn.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        rules_view.import_button.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("导入远程规则"), None);
            let group = adw::PreferencesGroup::new();
            let url = adw::EntryRow::builder()
                .title("HTTPS 订阅地址")
                .text(rule_source.text())
                .build();
            group.add(&url);
            dialog.set_extra_child(Some(&group));
            dialog.add_response("cancel", "取消");
            dialog.add_response("import", "导入");
            dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
            let trigger = trigger.clone();
            let rule_source = rule_source.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "import" {
                    rule_source.set_text(url.text().trim());
                    trigger.emit_clicked();
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let trigger = rules_view.import_trigger_btn.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        let config = config.clone();
        rules_view.update_source_btn.connect_clicked(move |_| {
            rule_source.set_text(&config.borrow().settings.rule_source_url);
            trigger.emit_clicked();
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let stack = rules_view.domain_stack.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.remove_source_btn.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("确认删除该订阅配置？"), None);
            dialog.add_response("cancel", "取消");
            dialog.add_response("remove", "删除");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let stack = stack.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "remove" {
                    let mut current = config.borrow_mut();
                    current.settings.imported_domain_rules.clear();
                    current.settings.imported_ip_rules.clear();
                    current.settings.rule_source_url.clear();
                    current.settings.rule_source_name.clear();
                    current.settings.rule_source_updated_at = 0;
                    if current.save().is_ok() {
                        drop(current);
                        stack.set_visible_child_name("overview");
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.clear_rules_btn.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("确认清空所有自定义规则？"), None);
            dialog.add_response("cancel", "取消");
            dialog.add_response("clear", "清空");
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "clear" {
                    let mut current = config.borrow_mut();
                    current.settings.domain_rules.clear();
                    current.settings.ip_rules.clear();
                    if current.save().is_ok() {
                        drop(current);
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.add_rule_btn.connect_clicked(move |_| {
            show_rule_dialog(&parent, config.clone(), None, refresh.clone());
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.import_omega_btn.connect_clicked(move |_| {
            let file_dialog = gtk::FileDialog::builder()
                .title("导入 SwitchyOmega 规则备份")
                .accept_label("打开")
                .build();

            let filter = gtk::FileFilter::new();
            filter.add_pattern("*.bak");
            filter.add_pattern("*.json");
            filter.set_name(Some("Omega 备份文件 (*.bak, *.json)"));

            let all_filter = gtk::FileFilter::new();
            all_filter.add_pattern("*");
            all_filter.set_name(Some("所有文件"));

            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            filters.append(&all_filter);
            file_dialog.set_filters(Some(&filters));

            let dialog_parent = parent.clone();
            let config = config.clone();
            let refresh = refresh.clone();
            file_dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                let parent = dialog_parent;
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("读取失败"),
                            Some(&format!("无法读取备份文件: {e}")),
                        );
                        dialog.add_response("ok", "确定");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let parsed = match parse_omega_rules(&content) {
                    Ok(p) => p,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("解析失败"),
                            Some(&format!("备份文件格式无效: {e}")),
                        );
                        dialog.add_response("ok", "确定");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let rule_count = parsed.rule_count();
                let domain_count = parsed.domain_rules.len();
                let ip_count = parsed.ip_rules.len();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("备份文件");

                let dialog = adw::AlertDialog::new(
                    Some("导入 Omega 规则"),
                    Some(&format!(
                        "在 \"{file_name}\" 中解析出 {rule_count} 条规则 ({domain_count} 域名, {ip_count} IP)。\n\n请选择导入方式:",
                    )),
                );
                dialog.add_response("cancel", "取消");
                dialog.add_response("replace", "完全替换");
                dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
                dialog.add_response("merge", "增量合并");
                dialog.set_response_appearance("merge", adw::ResponseAppearance::Suggested);

                let config = config.clone();
                let refresh = refresh.clone();
                let err_parent = parent.clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "cancel" {
                        return;
                    }
                    let mut current = config.borrow_mut();
                    if response == "replace" {
                        current.settings.domain_rules = parsed.domain_rules.clone();
                        current.settings.ip_rules = parsed.ip_rules.clone();
                    } else if response == "merge" {
                        for new_domain in &parsed.domain_rules {
                            if let Some(existing) = current
                                .settings
                                .domain_rules
                                .iter_mut()
                                .find(|item| item.pattern == new_domain.pattern && item.kind == new_domain.kind)
                            {
                                existing.action = new_domain.action;
                            } else {
                                current.settings.domain_rules.push(new_domain.clone());
                            }
                        }
                        for new_ip in &parsed.ip_rules {
                            if let Some(existing) = current
                                .settings
                                .ip_rules
                                .iter_mut()
                                .find(|item| item.network == new_ip.network)
                            {
                                existing.action = new_ip.action;
                            } else {
                                current.settings.ip_rules.push(new_ip.clone());
                            }
                        }
                    }

                    if let Err(e) = current.save() {
                        let err_dialog = adw::AlertDialog::new(
                            Some("保存失败"),
                            Some(&format!("规则写入失败: {e}")),
                        );
                        err_dialog.add_response("ok", "确定");
                        err_dialog.present(Some(&err_parent));
                        return;
                    }
                    drop(current);
                    if let Some(refresh) = refresh.borrow().as_ref() {
                        refresh();
                    }
                });

                dialog.present(Some(&parent));
            });
        });
    }

    // 7. 黑名单逻辑
    let on_add_proc = {
        let new_proc_row = rules_view.new_proc_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let proc_name = new_proc_row.text().trim().to_string();
            if proc_name.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            current.settings.app_rules.retain(|r| {
                r.executable.file_name().and_then(|n| n.to_str()) != Some(&proc_name)
            });
            current.settings.app_rules.push(AppRule {
                executable: PathBuf::from(&proc_name),
                action: RuleAction::Block,
            });
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_proc_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    {
        let on_add = on_add_proc.clone();
        rules_view
            .add_proc_btn
            .connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_proc.clone();
        rules_view
            .new_proc_row
            .connect_entry_activated(move |_| on_add());
    }

    let on_add_target = {
        let new_target_row = rules_view.new_target_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let target = new_target_row.text().trim().to_string();
            if target.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            if target.contains('/') || target.parse::<std::net::IpAddr>().is_ok() {
                let parsed = parse_rule_set(&format!("IP-CIDR,{target}"), RuleAction::Block);
                for rule in parsed.ip_rules {
                    current
                        .settings
                        .ip_rules
                        .retain(|r| r.network != rule.network);
                    current.settings.ip_rules.push(rule);
                }
            } else {
                let parsed = parse_rule_set(&format!("DOMAIN-SUFFIX,{target}"), RuleAction::Block);
                for rule in parsed.domain_rules {
                    current
                        .settings
                        .domain_rules
                        .retain(|r| !(r.pattern == rule.pattern && r.kind == rule.kind));
                    current.settings.domain_rules.push(rule);
                }
            }
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_target_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    {
        let on_add = on_add_target.clone();
        rules_view
            .add_target_btn
            .connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_target.clone();
        rules_view
            .new_target_row
            .connect_entry_activated(move |_| on_add());
    }

    for app in &scan_desktop_apps() {
        let row = adw::ActionRow::builder()
            .title(&app.name)
            .subtitle(&app.executable)
            .build();
        row.add_prefix(&create_app_icon(&app.icon));

        let sw = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let is_blocked = current_app_action(&config.borrow(), &app.executable) == RuleAction::Block;
        sw.set_active(is_blocked);

        let config_ref = config.clone();
        let controller_ref = controller.clone();
        let executable = app.executable.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        sw.connect_active_notify(move |sw| {
            let currently_blocked =
                current_app_action(&config_ref.borrow(), &executable) == RuleAction::Block;
            if sw.is_active() == currently_blocked {
                return;
            }
            let mut current = config_ref.borrow_mut();
            if sw.is_active() {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
                current.settings.app_rules.push(AppRule {
                    executable: PathBuf::from(&executable),
                    action: RuleAction::Block,
                });
            } else {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
            }
            let _ = current.save();
            drop(current);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        });

        row.add_suffix(&sw);
        row.set_activatable_widget(Some(&sw));
        rules_view.blocked_apps_list_box.append(&row);

        rules_view.app_switches.borrow_mut().push((
            format!("{} {}", app.name, app.executable).to_lowercase(),
            row,
            sw,
        ));
    }

    {
        let app_switches = rules_view.app_switches.clone();
        rules_view.app_search_row.connect_changed(move |entry| {
            let query = entry.text().trim().to_lowercase();
            for (key, row, _) in app_switches.borrow().iter() {
                row.set_visible(query.is_empty() || key.contains(&query));
            }
        });
    }

    let refresh_blocked_impl: Rc<dyn Fn()> = {
        let procs_list_box = rules_view.procs_list_box.clone();
        let blocked_targets_list_box = rules_view.blocked_targets_list_box.clone();
        let app_switches = rules_view.app_switches.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();

        Rc::new(move || {
            while let Some(child) = procs_list_box.first_child() {
                procs_list_box.remove(&child);
            }
            let current = config.borrow();
            let procs: Vec<String> = current
                .settings
                .app_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .filter_map(|r| {
                    r.executable
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(ToString::to_string)
                })
                .collect();

            while let Some(child) = blocked_targets_list_box.first_child() {
                blocked_targets_list_box.remove(&child);
            }
            let blocked_domains: Vec<DomainRule> = current
                .settings
                .domain_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            let blocked_ips: Vec<IpRule> = current
                .settings
                .ip_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            drop(current);

            for proc_name in procs {
                let row = adw::ActionRow::builder()
                    .title(&proc_name)
                    .subtitle("已禁止外部网络连接")
                    .build();
                let icon = gtk::Image::from_icon_name("network-offline-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let target = proc_name.clone();
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.app_rules.retain(|r| {
                        r.executable.file_name().and_then(|n| n.to_str()) != Some(&target)
                    });
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                procs_list_box.append(&row);
            }

            for rule in blocked_domains {
                let row = adw::ActionRow::builder()
                    .title(&rule.pattern)
                    .subtitle(&format!("{} · 拦截", domain_kind_label(rule.kind)))
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let pattern = rule.pattern.clone();
                let kind = rule.kind;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current
                        .settings
                        .domain_rules
                        .retain(|r| !(r.pattern == pattern && r.kind == kind));
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_list_box.append(&row);
            }

            for rule in blocked_ips {
                let row = adw::ActionRow::builder()
                    .title(&rule.network.to_string())
                    .subtitle("IP-CIDR · 拦截")
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let network = rule.network;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.ip_rules.retain(|r| r.network != network);
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_list_box.append(&row);
            }

            for (_, row, sw) in app_switches.borrow().iter() {
                if let Some(exec) = row.subtitle().map(|s| s.to_string()) {
                    let is_blocked =
                        current_app_action(&config.borrow(), &exec) == RuleAction::Block;
                    if sw.is_active() != is_blocked {
                        sw.set_active(is_blocked);
                    }
                }
            }
        })
    };
    *refresh_blocked_views.borrow_mut() = Some(refresh_blocked_impl.clone());
    refresh_blocked_impl();

    // 8. 流量监控逻辑
    let refresh_traffic_rule_counts_impl = {
        let config = config.clone();
        let total_rules_label = traffic_view.total_rules_label.clone();
        let proxy_seg = traffic_view.proxy_seg.clone();
        let reject_seg = traffic_view.reject_seg.clone();
        let direct_seg = traffic_view.direct_seg.clone();
        let proxy_legend_label = traffic_view.proxy_legend_label.clone();
        let reject_legend_label = traffic_view.reject_legend_label.clone();
        let direct_legend_label = traffic_view.direct_legend_label.clone();
        Rc::new(move || {
            refresh_traffic_rule_counts(
                &config,
                &total_rules_label,
                &proxy_seg,
                &reject_seg,
                &direct_seg,
                &proxy_legend_label,
                &reject_legend_label,
                &direct_legend_label,
            );
        })
    };
    *refresh_traffic_rule_counts_fn.borrow_mut() =
        Some(refresh_traffic_rule_counts_impl.clone());
    refresh_traffic_rule_counts_impl();

    let app_traffic_data = Rc::new(RefCell::new(Vec::<AppTrafficStat>::new()));
    let refresh_app_traffic = {
        let app_traffic_data = app_traffic_data.clone();
        let app_traffic_search = traffic_view.app_traffic_search.clone();
        let app_traffic_sort = traffic_view.app_traffic_sort.clone();
        let app_traffic_list_box = traffic_view.app_traffic_list_box.clone();
        Rc::new(move || {
            refresh_app_traffic_list(
                &app_traffic_data,
                &app_traffic_search,
                &app_traffic_sort,
                &app_traffic_list_box,
            );
        })
    };
    {
        let refresh = refresh_app_traffic.clone();
        traffic_view
            .app_traffic_search
            .connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_app_traffic.clone();
        traffic_view
            .app_traffic_sort
            .connect_selected_notify(move |_| refresh());
    }

    // 9. 日志视图逻辑
    {
        let all_buffer = logs_view.all_log_buffer.clone();
        let system_buffer = logs_view.system_log_buffer.clone();
        let proxy_buffer = logs_view.proxy_log_buffer.clone();
        let direct_buffer = logs_view.direct_log_buffer.clone();
        let stack = logs_view.log_stack.clone();
        logs_view.clear_logs_btn.connect_clicked(move |_| {
            match stack.visible_child_name().as_deref() {
                Some("all") => all_buffer.set_text(""),
                Some("system") => system_buffer.set_text(""),
                Some("direct") => direct_buffer.set_text(""),
                _ => proxy_buffer.set_text(""),
            }
        });
    }
    {
        let all_buffer = logs_view.all_log_buffer.clone();
        let system_buffer = logs_view.system_log_buffer.clone();
        let proxy_buffer = logs_view.proxy_log_buffer.clone();
        let direct_buffer = logs_view.direct_log_buffer.clone();
        let stack = logs_view.log_stack.clone();
        logs_view.copy_logs_btn.connect_clicked(move |_| {
            let target_buffer = match stack.visible_child_name().as_deref() {
                Some("all") => &all_buffer,
                Some("system") => &system_buffer,
                Some("direct") => &direct_buffer,
                _ => &proxy_buffer,
            };
            let text =
                target_buffer.text(&target_buffer.start_iter(), &target_buffer.end_iter(), false);
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        });
    }

    // 10. 托盘初始化
    {
        let profiles_config = config.clone();
        let select_config = config.clone();
        let select_refresh = refresh_connections.clone();
        let select_connected = is_connected.clone();
        let toggle_config = config.clone();
        let toggle_buttons = connection_buttons.clone();
        let show_window = win.window.clone();
        let quit_app = app.clone();
        let quit_controller = controller.clone();
        let quitting_ref = quitting.clone();
        let log_buffer_ref = logs_view.proxy_log_buffer.clone();
        match TrayManager::new(
            Rc::new(move || {
                let current = profiles_config.borrow();
                current
                    .profiles
                    .iter()
                    .map(|profile| {
                        (
                            profile.name.clone(),
                            current.active_profile == Some(profile.id),
                        )
                    })
                    .collect()
            }),
            Rc::new(move |index| {
                if *select_connected.borrow() {
                    return;
                }
                let mut current = select_config.borrow_mut();
                let Some(profile_id) = current.profiles.get(index).map(|profile| profile.id) else {
                    return;
                };
                current.active_profile = Some(profile_id);
                if current.save().is_ok() {
                    drop(current);
                    if let Some(refresh) = select_refresh.borrow().as_ref() {
                        refresh();
                    }
                }
            }),
            Rc::new(move || {
                let active_id = toggle_config.borrow().active_profile.map(|id| id.to_string());
                let Some(active_id) = active_id else {
                    return;
                };
                if let Some((_, button)) = toggle_buttons
                    .borrow()
                    .iter()
                    .find(|(profile_id, _)| profile_id == &active_id)
                {
                    button.emit_clicked();
                }
            }),
            Rc::new(move || show_window.present()),
            Rc::new(move || {
                *quitting_ref.borrow_mut() = true;
                quit_controller.borrow().shutdown();
                quit_app.quit();
            }),
        ) {
            Ok(manager) => {
                *tray_manager.borrow_mut() = Some(Rc::new(manager));
            }
            Err(error) => {
                let mut end = log_buffer_ref.end_iter();
                log_buffer_ref.insert(&mut end, &format!("[tray] {error}\n"));
            }
        }
    }

    {
        let tray_manager = tray_manager.clone();
        let quitting = quitting.clone();
        win.window.connect_close_request(move |window| {
            if !*quitting.borrow() && tray_manager.borrow().is_some() {
                window.set_visible(false);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
    }

    // 11. 远程规则下载触发
    {
        let event_tx = event_tx.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        let import_rules = rules_view.import_trigger_btn.clone();
        let import_button = rules_view.import_button.clone();
        let rule_status = rules_view.rule_status_row.clone();
        import_rules.clone().connect_clicked(move |_| {
            let url = rule_source.text().trim().to_string();
            if url.is_empty() {
                rule_status.set_subtitle("订阅地址不能为空");
                return;
            }
            import_rules.set_sensitive(false);
            import_button.set_sensitive(false);
            rule_status.set_subtitle("正在下载并解析规则…");
            let events = event_tx.clone();
            thread::spawn(move || match import_rule_source(&url) {
                Ok(result) => {
                    let _ = events.send(RuntimeEvent::RulesImported {
                        result,
                        source_url: url,
                    });
                }
                Err(error) => {
                    let _ = events.send(RuntimeEvent::RuleImportFailed(error));
                }
            });
        });
    }

    // 12. 运行时事件总线轮询
    let session_upload = Rc::new(RefCell::new(0_u64));
    let session_download = Rc::new(RefCell::new(0_u64));

    {
        let is_connected = is_connected.clone();
        let config = config.clone();
        let policy = rules_view.policy_row.clone();
        let rule_status = rules_view.rule_status_row.clone();
        let import_rules = rules_view.import_trigger_btn.clone();
        let import_button = rules_view.import_button.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        let refresh_connections = refresh_connections.clone();
        let proxy_log_buffer = logs_view.proxy_log_buffer.clone();
        let proxy_log_view = logs_view.proxy_log_view.clone();
        let direct_log_buffer = logs_view.direct_log_buffer.clone();
        let direct_log_view = logs_view.direct_log_view.clone();
        let system_log_buffer = logs_view.system_log_buffer.clone();
        let system_log_view = logs_view.system_log_view.clone();
        let all_log_buffer = logs_view.all_log_buffer.clone();
        let all_log_view = logs_view.all_log_view.clone();
        let bottom_status_dot = win.bottom_status_dot.clone();
        let bottom_status = win.bottom_status.clone();
        let speed_label = win.speed_label.clone();
        let started_label = traffic_view.started_label.clone();
        let duration_label = traffic_view.duration_label.clone();
        let connect_start_time = connect_start_time.clone();
        let total_hero_label = traffic_view.total_hero_label.clone();
        let total_up_label = traffic_view.total_up_label.clone();
        let total_down_label = traffic_view.total_down_label.clone();
        let proxy_hero_label = traffic_view.proxy_hero_label.clone();
        let proxy_up_label = traffic_view.proxy_up_label.clone();
        let proxy_down_label = traffic_view.proxy_down_label.clone();
        let direct_hero_label = traffic_view.direct_hero_label.clone();
        let direct_up_label = traffic_view.direct_up_label.clone();
        let direct_down_label = traffic_view.direct_down_label.clone();
        let session_upload = session_upload.clone();
        let session_download = session_download.clone();
        let tray_manager = tray_manager.clone();
        let controller_ref = controller.clone();
        let app_traffic_data = app_traffic_data.clone();
        let refresh_app_traffic = refresh_app_traffic.clone();

        let update_traffic_labels = {
            let session_upload = session_upload.clone();
            let session_download = session_download.clone();
            let app_traffic_data = app_traffic_data.clone();
            let total_hero_label = total_hero_label.clone();
            let total_up_label = total_up_label.clone();
            let total_down_label = total_down_label.clone();
            let proxy_hero_label = proxy_hero_label.clone();
            let proxy_up_label = proxy_up_label.clone();
            let proxy_down_label = proxy_down_label.clone();
            let direct_hero_label = direct_hero_label.clone();
            let direct_up_label = direct_up_label.clone();
            let direct_down_label = direct_down_label.clone();
            Rc::new(move || {
                let up_proxy = *session_upload.borrow();
                let down_proxy = *session_download.borrow();
                let (apps_up, apps_down) =
                    app_traffic_data
                        .borrow()
                        .iter()
                        .fold((0u64, 0u64), |(u, d), app| {
                            (u + app.upload, d + app.download)
                        });
                let total_up = up_proxy.max(apps_up);
                let total_down = down_proxy.max(apps_down);
                let direct_up = total_up.saturating_sub(up_proxy);
                let direct_down = total_down.saturating_sub(down_proxy);

                total_hero_label.set_text(&format_bytes(total_up + total_down));
                total_up_label.set_text(&format_bytes(total_up));
                total_down_label.set_text(&format_bytes(total_down));

                proxy_hero_label.set_text(&format_bytes(up_proxy + down_proxy));
                proxy_up_label.set_text(&format_bytes(up_proxy));
                proxy_down_label.set_text(&format_bytes(down_proxy));

                direct_hero_label.set_text(&format_bytes(direct_up + direct_down));
                direct_up_label.set_text(&format_bytes(direct_up));
                direct_down_label.set_text(&format_bytes(direct_down));
            })
        };

        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    RuntimeEvent::Connected => {
                        *session_upload.borrow_mut() = 0;
                        *session_download.borrow_mut() = 0;
                        app_traffic_data.borrow_mut().clear();
                        refresh_app_traffic();
                        update_traffic_labels();
                        *connect_start_time.borrow_mut() = Some(std::time::Instant::now());
                        if let Ok(now) = gtk::glib::DateTime::now_local() {
                            started_label.set_text(
                                &now.format("%Y-%m-%d %H:%M:%S")
                                    .map_or_else(|_| "—".into(), |s| s.to_string()),
                            );
                        }
                        duration_label.set_text("00:00:00");
                        *is_connected.borrow_mut() = true;

                        bottom_status_dot.remove_css_class("status-dot-disconnected");
                        bottom_status_dot.remove_css_class("status-dot-connecting");
                        bottom_status_dot.add_css_class("status-dot-connected");
                        bottom_status.set_text("已连接");

                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Connected);
                        }
                        if let Some(refresh) = refresh_connections.borrow().as_ref() {
                            refresh();
                        }
                    }
                    RuntimeEvent::Disconnected => {
                        *is_connected.borrow_mut() = false;
                        *connect_start_time.borrow_mut() = None;

                        bottom_status_dot.remove_css_class("status-dot-connected");
                        bottom_status_dot.remove_css_class("status-dot-connecting");
                        bottom_status_dot.add_css_class("status-dot-disconnected");
                        bottom_status.set_text("未连接");

                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Disconnected);
                        }
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        if let Some(refresh) = refresh_connections.borrow().as_ref() {
                            refresh();
                        }
                    }
                    RuntimeEvent::Status(status) => {
                        bottom_status_dot.remove_css_class("status-dot-connected");
                        bottom_status_dot.remove_css_class("status-dot-disconnected");
                        bottom_status_dot.add_css_class("status-dot-connecting");
                        bottom_status.set_text(&status);

                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Connecting);
                        }
                        if let Some(refresh) = refresh_connections.borrow().as_ref() {
                            refresh();
                        }
                    }
                    RuntimeEvent::Error(error) => {
                        *is_connected.borrow_mut() = false;
                        *connect_start_time.borrow_mut() = None;

                        bottom_status_dot.remove_css_class("status-dot-connected");
                        bottom_status_dot.remove_css_class("status-dot-connecting");
                        bottom_status_dot.add_css_class("status-dot-disconnected");
                        bottom_status.set_text(&error);

                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Disconnected);
                        }
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        if let Some(refresh) = refresh_connections.borrow().as_ref() {
                            refresh();
                        }
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                    }
                    RuntimeEvent::Log(line) => {
                        let time_str = gtk::glib::DateTime::now_local()
                            .and_then(|dt| dt.format("%H:%M:%S"))
                            .map(|gstr| gstr.to_string())
                            .unwrap_or_else(|_| "00:00:00".to_string());
                        let formatted = format!("[{time_str}] {line}\n");

                        let append_to = |buffer: &gtk::TextBuffer, view: &gtk::TextView| {
                            let mut end = buffer.end_iter();
                            buffer.insert(&mut end, &formatted);
                            let end = buffer.end_iter();
                            let mark = buffer.create_mark(None, &end, false);
                            view.scroll_mark_onscreen(&mark);
                        };

                        append_to(&all_log_buffer, &all_log_view);

                        let is_proxy = line.contains("[proxy]") || line.contains("-> Proxy");
                        let is_direct = line.contains("[direct]")
                            || line.contains("-> Direct")
                            || line.contains("Direct (");
                        if is_proxy {
                            append_to(&proxy_log_buffer, &proxy_log_view);
                        } else if is_direct {
                            append_to(&direct_log_buffer, &direct_log_view);
                        } else {
                            append_to(&system_log_buffer, &system_log_view);
                        }
                    }
                    RuntimeEvent::Speed { upload, download } => {
                        *session_upload.borrow_mut() += upload;
                        *session_download.borrow_mut() += download;
                        let up_total = *session_upload.borrow();
                        let down_total = *session_download.borrow();
                        update_traffic_labels();
                        speed_label.set_text(&format!(
                            "↑ {} ({})   ↓ {} ({})",
                            format_speed(upload),
                            format_bytes(up_total),
                            format_speed(download),
                            format_bytes(down_total)
                        ));
                    }
                    RuntimeEvent::AppTraffic(stats) => {
                        *app_traffic_data.borrow_mut() = stats;
                        refresh_app_traffic();
                        update_traffic_labels();
                    }
                    RuntimeEvent::RuleImportFailed(error) => {
                        rule_status.set_subtitle(&format!("导入失败: {error}"));
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                    }
                    RuntimeEvent::RulesImported { result, source_url } => {
                        let (direct, proxy, block) = result.action_counts();
                        let total = result.rule_count();
                        {
                            let mut current = config.borrow_mut();
                            current.settings.imported_domain_rules = result.domain_rules;
                            current.settings.imported_ip_rules = result.ip_rules;
                            current.settings.default_policy = result.default_policy;
                            current.settings.rule_source_name = rule_source_name(&source_url);
                            current.settings.rule_source_url = source_url;
                            current.settings.rule_source_updated_at = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |duration| duration.as_secs() as i64);
                            if let Err(error) = current.save() {
                                rule_status.set_subtitle(&format!("配置保存失败: {error}"));
                                import_rules.set_sensitive(true);
                                import_button.set_sensitive(true);
                                continue;
                            }
                        }
                        policy.set_selected(match result.default_policy {
                            RuleAction::Proxy => 0,
                            RuleAction::Direct => 1,
                            RuleAction::Block => 2,
                        });
                        rule_status.set_subtitle(&format!(
                            "共 {total} 条规则 · 直连 {direct} · 代理 {proxy} · 拦截 {block} · 跳过 {}",
                            result.ignored_count
                        ));
                        let mut end = proxy_log_buffer.end_iter();
                        proxy_log_buffer.insert(
                            &mut end,
                            &format!(
                                "[rules] 成功导入 {total} 条规则 (直连 {direct}, 代理 {proxy}, 拦截 {block})\n"
                            ),
                        );
                        for warning in result.warnings.iter().take(3) {
                            let mut end = proxy_log_buffer.end_iter();
                            proxy_log_buffer.insert(&mut end, &format!("[rules] 警告: {warning}\n"));
                        }
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                        if let Some(refresh) = refresh_rule_views.borrow().as_ref() {
                            refresh();
                        }
                        controller_ref.borrow().sync_rules();
                    }
                }
            }
            if let Some(start) = *connect_start_time.borrow() {
                let elapsed = start.elapsed().as_secs();
                duration_label.set_text(&format_duration(elapsed));
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    win.window.present();
}

fn main() {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run();
}

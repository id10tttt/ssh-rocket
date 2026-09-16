use anyhow::{Context, Result, bail};
use ipnet::IpNet;
use ssh_rocket_core::{AppConfig, FlowContext, RoutingEngine, RuleAction};
use ssh_rocket_runtime::ipc::{HelperCommand, HelperEvent};
use std::{
    collections::HashMap,
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UdpSocket,
    process::Command,
    signal,
    task::JoinHandle,
    time::{interval, sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use tun2proxy::{ArgDns, ArgProxy, Args};

const TUN_NAME: &str = "sshrocket0";
const MARK: &str = "0x5352";
const TABLE: &str = "21330";
const NFT_TABLE: &str = "ssh_rocket";
const DNS_LISTEN_PORT: u16 = 15353;
const MAX_TUN_RETRIES: usize = 3;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if unsafe { libc::geteuid() } != 0 {
        bail!("ssh-rocket-helper must run as root");
    }
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to monitor the GUI process");
    }
    if unsafe { libc::getppid() } == 1 {
        bail!("parent process exited before the helper started");
    }

    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "daemon".to_string());

    if command == "daemon" {
        run_daemon().await
    } else if command == "run" {
        let config_path = args.next().context("missing config path")?;
        let uid: u32 = args.next().context("missing uid")?.parse().context("invalid uid")?;
        let socks_port: u16 = args.next().context("missing SOCKS port")?.parse().context("invalid SOCKS port")?;
        let dns_port: u16 = args.next().context("missing DNS port")?.parse().context("invalid DNS port")?;
        let ssh_port: u16 = args.next().context("missing SSH port")?.parse().context("invalid SSH port")?;
        let ssh_addresses = args
            .map(|value| value.parse::<IpAddr>().context("invalid SSH address"))
            .collect::<Result<Vec<_>>>()?;
        if ssh_addresses.is_empty() {
            bail!("at least one SSH server address is required");
        }
        run_oneshot(
            PathBuf::from(config_path),
            uid,
            socks_port,
            dns_port,
            ssh_port,
            ssh_addresses,
        )
        .await
    } else {
        bail!("usage: ssh-rocket-helper daemon | run <config.json> <uid> <socks-port> <dns-port> <ssh-port> <ssh-address>...");
    }
}

struct ActiveSession {
    shutdown: CancellationToken,
    tun_task: JoinHandle<std::io::Result<usize>>,
    dns_task: JoinHandle<Result<()>>,
    system: SystemState,
}

impl ActiveSession {
    async fn stop(self) {
        self.shutdown.cancel();
        if !self.dns_task.is_finished() {
            let _ = self.dns_task.await;
        }
        if !self.tun_task.is_finished() {
            let _ = self.tun_task.await;
        }
        self.system.cleanup().await;
        clean_stale_resources().await;
        eprintln!("[helper] routing stopped");
    }
}

async fn run_daemon() -> Result<()> {
    eprintln!("[helper] starting helper daemon");
    clean_stale_resources().await;

    let mut stdout = tokio::io::stdout();
    let stdin = tokio::io::stdin();
    let mut stdin_lines = BufReader::new(stdin).lines();

    send_event(&mut stdout, &HelperEvent::Ready).await?;

    let mut active_session: Option<ActiveSession> = None;
    let mut app_scan = interval(Duration::from_secs(2));

    let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
        .context("failed to listen for termination")?;
    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                eprintln!("[helper] received interrupt, exiting");
                break;
            }
            _ = terminate.recv() => {
                eprintln!("[helper] received SIGTERM, exiting");
                break;
            }
            line = stdin_lines.next_line() => {
                let Some(line) = line? else {
                    eprintln!("[helper] stdin closed by client, exiting");
                    break;
                };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let command: HelperCommand = match serde_json::from_str(line) {
                    Ok(cmd) => cmd,
                    Err(err) => {
                        eprintln!("[helper] invalid JSON command: {err}");
                        send_event(&mut stdout, &HelperEvent::Error { message: format!("invalid command: {err}") }).await?;
                        continue;
                    }
                };

                match command {
                    HelperCommand::Start {
                        config_path,
                        uid,
                        socks_port,
                        dns_port,
                        ssh_port,
                        ssh_addresses,
                    } => {
                        if let Some(session) = active_session.take() {
                            session.stop().await;
                        }
                        match start_proxy_session(
                            config_path,
                            uid,
                            socks_port,
                            dns_port,
                            ssh_port,
                            ssh_addresses,
                        ).await {
                            Ok(session) => {
                                active_session = Some(session);
                                send_event(&mut stdout, &HelperEvent::Active).await?;
                            }
                            Err(err) => {
                                eprintln!("[helper] failed to start proxy session: {err}");
                                clean_stale_resources().await;
                                send_event(&mut stdout, &HelperEvent::Error { message: err.to_string() }).await?;
                            }
                        }
                    }
                    HelperCommand::Stop => {
                        if let Some(session) = active_session.take() {
                            session.stop().await;
                        } else {
                            clean_stale_resources().await;
                        }
                        send_event(&mut stdout, &HelperEvent::Stopped).await?;
                    }
                    HelperCommand::SyncRules => {
                        if let Some(session) = active_session.as_mut() {
                            if let Err(err) = session.system.assign_apps().await {
                                eprintln!("[helper] failed to sync app rules: {err}");
                            }
                        }
                        send_event(&mut stdout, &HelperEvent::RulesSynced).await?;
                    }
                    HelperCommand::Status => {
                        let unhealthy = active_session.as_ref().is_some_and(|session| {
                            session.tun_task.is_finished() || session.dns_task.is_finished()
                        });
                        if unhealthy {
                            eprintln!("[helper] active routing task exited unexpectedly");
                            if let Some(session) = active_session.take() {
                                session.stop().await;
                            }
                            send_event(
                                &mut stdout,
                                &HelperEvent::Error {
                                    message: "transparent proxy routing task exited unexpectedly".into(),
                                },
                            )
                            .await?;
                        } else {
                            send_event(
                                &mut stdout,
                                &HelperEvent::Status {
                                    active: active_session.is_some(),
                                },
                            )
                            .await?;
                        }
                    }
                    HelperCommand::Quit => {
                        eprintln!("[helper] quit command received");
                        break;
                    }
                }
            }
            tun_res = async {
                match active_session.as_mut() {
                    Some(session) => (&mut session.tun_task).await,
                    None => std::future::pending().await,
                }
            } => {
                let msg = match tun_res {
                    Ok(Err(e)) => format!("tun runtime failed: {e}"),
                    Ok(Ok(_)) => "tun runtime exited".to_string(),
                    Err(e) => format!("tun task panicked: {e}"),
                };
                eprintln!("[helper] {msg}");
                if let Some(session) = active_session.take() {
                    session.stop().await;
                }
                send_event(&mut stdout, &HelperEvent::Error { message: msg }).await?;
            }
            _ = app_scan.tick() => {
                if unsafe { libc::getppid() } == 1 {
                    eprintln!("[helper] parent process exited (orphaned), exiting daemon");
                    break;
                }
                if let Some(session) = active_session.as_mut() {
                    if let Err(error) = session.system.assign_apps().await {
                        eprintln!("app rule synchronization failed: {error}");
                    }
                }
            }
        }
    }

    if let Some(session) = active_session.take() {
        session.stop().await;
    }
    clean_stale_resources().await;
    eprintln!("[helper] helper daemon exited cleanly");
    Ok(())
}

async fn send_event<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    event: &HelperEvent,
) -> Result<()> {
    let mut json = serde_json::to_string(event)?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// 启动透明代理会话。严格遵循顺序：清理残留 -> 创建 TUN -> 确认 tun2proxy 健康 -> 启用系统 routing -> 失败原子回滚
async fn start_proxy_session(
    config_path: PathBuf,
    uid: u32,
    socks_port: u16,
    dns_port: u16,
    ssh_port: u16,
    ssh_addresses: Vec<IpAddr>,
) -> Result<ActiveSession> {
    eprintln!("[helper] starting transparent proxy");
    let config: AppConfig = serde_json::from_slice(&tokio::fs::read(&config_path).await?)?;
    let proxy = ArgProxy::try_from(format!("socks5://127.0.0.1:{socks_port}").as_str())?;
    let mut proxy_args = Args::default();
    proxy_args
        .proxy(proxy)
        .tun(TUN_NAME.to_string())
        .dns(ArgDns::Virtual)
        .ipv6_enabled(config.settings.ipv6)
        .setup(false);

    // 1. 创建 TUN 并重试解决 EBUSY
    let mut tun_task = None;
    let mut shutdown = CancellationToken::new();
    let mut last_error = String::new();

    for attempt in 1..=MAX_TUN_RETRIES {
        clean_stale_resources().await;
        sleep(Duration::from_millis(100)).await;

        let cur_shutdown = CancellationToken::new();
        let tun_shutdown_clone = cur_shutdown.clone();
        let cur_args = proxy_args.clone();

        let mut task = tokio::spawn(async move {
            tun2proxy::general_run_async(cur_args, 1500, true, tun_shutdown_clone).await
        });

        let startup_check = tokio::select! {
            res = &mut task => {
                match res {
                    Ok(Err(e)) => Err(e.to_string()),
                    Ok(Ok(_)) => Err("tun2proxy terminated prematurely".to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            res = wait_for_interface() => {
                match res {
                    Ok(_) => {
                        // 确认设备健康建立且未立即崩溃
                        sleep(Duration::from_millis(150)).await;
                        if task.is_finished() {
                            match (&mut task).await {
                                Ok(Err(e)) => Err(e.to_string()),
                                Ok(Ok(_)) => Err("tun2proxy closed immediately".to_string()),
                                Err(e) => Err(e.to_string()),
                            }
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            _ = sleep(Duration::from_secs(4)) => {
                Err(format!("timed out waiting for interface {TUN_NAME}"))
            }
        };

        match startup_check {
            Ok(()) => {
                tun_task = Some(task);
                shutdown = cur_shutdown;
                eprintln!("[helper] TUN interface {TUN_NAME} ready");
                break;
            }
            Err(err) => {
                cur_shutdown.cancel();
                let _ = (&mut task).await;
                last_error = err;
                let is_busy = last_error.contains("Device or resource busy") || last_error.contains("os error 16");
                if is_busy && attempt < MAX_TUN_RETRIES {
                    eprintln!(
                        "[helper] TUN interface {TUN_NAME} is busy (os error 16), cleaning up and retrying in 1s (attempt {attempt}/{MAX_TUN_RETRIES})..."
                    );
                    clean_stale_resources().await;
                    sleep(Duration::from_millis(1000)).await;
                } else if attempt < MAX_TUN_RETRIES {
                    eprintln!(
                        "[helper] TUN startup attempt {attempt}/{MAX_TUN_RETRIES} failed: {last_error}, retrying in 1s..."
                    );
                    clean_stale_resources().await;
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    }

    let Some(tun_task) = tun_task else {
        clean_stale_resources().await;
        bail!("failed to create TUN {TUN_NAME} after {MAX_TUN_RETRIES} attempts: {last_error}");
    };

    // 2. 绑定 DNS 路由
    let dns_socket = match UdpSocket::bind(("0.0.0.0", DNS_LISTEN_PORT)).await {
        Ok(socket) => socket,
        Err(error) => {
            shutdown.cancel();
            let _ = tun_task.await;
            clean_stale_resources().await;
            return Err(error).context("failed to bind local DNS router");
        }
    };

    // 3. 启用系统 routing
    let mut system = SystemState::new(
        uid,
        socks_port,
        dns_port,
        ssh_port,
        ssh_addresses,
        config_path,
        config.clone(),
    );
    if let Err(error) = system.setup().await {
        eprintln!("[helper] system routing setup failed: {error}");
        shutdown.cancel();
        let _ = tun_task.await;
        system.cleanup().await;
        clean_stale_resources().await;
        return Err(error);
    }
    eprintln!("[helper] routing is active on {TUN_NAME}");

    let dns_shutdown = shutdown.clone();
    let routing_engine = RoutingEngine::new(config.settings.clone());
    let ipv6_enabled = config.settings.ipv6;
    let dns_task = tokio::spawn(async move {
        run_dns_router(dns_socket, dns_port, routing_engine, ipv6_enabled, dns_shutdown).await
    });

    Ok(ActiveSession {
        shutdown,
        tun_task,
        dns_task,
        system,
    })
}

async fn run_oneshot(
    config_path: PathBuf,
    uid: u32,
    socks_port: u16,
    dns_port: u16,
    ssh_port: u16,
    ssh_addresses: Vec<IpAddr>,
) -> Result<()> {
    let session = start_proxy_session(
        config_path,
        uid,
        socks_port,
        dns_port,
        ssh_port,
        ssh_addresses,
    )
    .await?;

    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
        .context("failed to listen for termination")?;

    let mut session = session;
    let mut app_scan = interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = terminate.recv() => break,
            tun_res = &mut session.tun_task => {
                match tun_res {
                    Ok(Ok(_)) => {},
                    Ok(Err(error)) => eprintln!("tun runtime failed: {error}"),
                    Err(error) => eprintln!("tun task failed: {error}"),
                }
                break;
            }
            _ = app_scan.tick() => {
                if unsafe { libc::getppid() } == 1 {
                    eprintln!("[helper] parent process exited (orphaned), exiting oneshot");
                    break;
                }
                if let Err(error) = session.system.assign_apps().await {
                    eprintln!("app rule synchronization failed: {error}");
                }
            }
        }
    }

    session.stop().await;
    Ok(())
}

async fn clean_stale_resources() {
    let my_pid = std::process::id();
    if let Ok(mut entries) = tokio::fs::read_dir("/proc").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue; };
            if pid == my_pid { continue; }
            if let Ok(cmdline) = tokio::fs::read_to_string(format!("/proc/{pid}/cmdline")).await {
                if cmdline.contains("ssh-rocket-helper") {
                    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                }
            }
        }
    }
    let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
    let _ = command("ip", &["-4", "rule", "del", "priority", "21329"]).await;
    let _ = command("ip", &["-4", "rule", "del", "priority", TABLE]).await;
    let _ = command("ip", &["-6", "rule", "del", "priority", TABLE]).await;
    let _ = command("ip", &["-4", "route", "flush", "table", TABLE]).await;
    let _ = command("ip", &["-6", "route", "flush", "table", TABLE]).await;
    let _ = command("ip", &["link", "delete", "dev", TUN_NAME]).await;
    for group in ["sshrocket-proxy", "sshrocket-direct", "sshrocket-block"] {
        if let Ok(pids) = tokio::fs::read_to_string(format!("/sys/fs/cgroup/{group}/cgroup.procs")).await {
            for pid in pids.lines() {
                let _ = tokio::fs::write("/sys/fs/cgroup/cgroup.procs", pid).await;
            }
        }
        let _ = tokio::fs::remove_dir(format!("/sys/fs/cgroup/{group}")).await;
    }
}

struct SystemState {
    uid: u32,
    socks_port: u16,
    dns_port: u16,
    ssh_port: u16,
    ssh_addresses: Vec<IpAddr>,
    config_path: PathBuf,
    config: AppConfig,
}

impl SystemState {
    fn new(
        uid: u32,
        socks_port: u16,
        dns_port: u16,
        ssh_port: u16,
        ssh_addresses: Vec<IpAddr>,
        config_path: PathBuf,
        config: AppConfig,
    ) -> Self {
        Self {
            uid,
            socks_port,
            dns_port,
            ssh_port,
            ssh_addresses,
            config_path,
            config,
        }
    }

    async fn setup(&mut self) -> Result<()> {
        let _ = self.run_ip(&["-4", "rule", "del", "priority", "21329"]).await;
        let _ = self.run_ip(&["-4", "rule", "del", "priority", TABLE]).await;
        let _ = self.run_ip(&["-6", "rule", "del", "priority", TABLE]).await;
        self.run_ip(&["link", "set", "dev", TUN_NAME, "up"]).await?;
        let _ = self.run_ip(&["-4", "addr", "replace", "10.0.0.33/24", "dev", TUN_NAME]).await;
        let _ = self.run_ip(&["-4", "route", "replace", "10.0.0.0/24", "dev", TUN_NAME]).await;
        let _ = self.run_ip(&["-4", "route", "replace", "198.18.0.0/15", "dev", TUN_NAME, "table", TABLE]).await;
        self.run_ip(&["-4", "route", "replace", "default", "dev", TUN_NAME, "table", TABLE]).await?;
        self.run_ip(&["-4", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE]).await?;
        let _ = self.run_ip(&["-4", "rule", "add", "priority", "21329", "to", "198.18.0.0/15", "lookup", TABLE]).await;
        if self.config.settings.ipv6 {
            self.run_ip(&["-6", "route", "replace", "default", "dev", TUN_NAME, "table", TABLE]).await?;
            self.run_ip(&["-6", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE]).await?;
        }
        self.setup_cgroups().await?;
        self.install_nftables().await?;
        self.assign_apps().await?;
        Ok(())
    }

    async fn cleanup(&self) {
        let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
        let _ = command("ip", &["-4", "rule", "del", "priority", "21329"]).await;
        let _ = command("ip", &["-4", "rule", "del", "priority", TABLE]).await;
        let _ = command("ip", &["-6", "rule", "del", "priority", TABLE]).await;
        let _ = command("ip", &["-4", "route", "flush", "table", TABLE]).await;
        let _ = command("ip", &["-6", "route", "flush", "table", TABLE]).await;
        self.restore_app_processes().await;
        for name in ["sshrocket-proxy", "sshrocket-direct", "sshrocket-block"] {
            let path = format!("/sys/fs/cgroup/{name}");
            let _ = tokio::fs::remove_dir(path).await;
        }
    }

    async fn run_ip(&self, args: &[&str]) -> Result<()> {
        command("ip", args).await
    }

    async fn setup_cgroups(&self) -> Result<()> {
        for name in ["sshrocket-proxy", "sshrocket-direct", "sshrocket-block"] {
            tokio::fs::create_dir_all(format!("/sys/fs/cgroup/{name}"))
                .await
                .with_context(|| format!("failed to create cgroup {name}"))?;
        }
        Ok(())
    }

    async fn assign_apps(&mut self) -> Result<()> {
        if let Ok(bytes) = tokio::fs::read(&self.config_path).await {
            if let Ok(config) = serde_json::from_slice::<AppConfig>(&bytes) {
                self.config.settings.app_rules = config.settings.app_rules;
            }
        }
        self.restore_app_processes().await;
        let rules: HashMap<_, _> = self
            .config
            .settings
            .app_rules
            .iter()
            .map(|rule| (rule.executable.clone(), rule.action))
            .collect();
        if rules.is_empty() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir("/proc").await?;
        while let Some(entry) = entries.next_entry().await? {
            let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else { continue; };
            let status = tokio::fs::read_to_string(format!("/proc/{pid}/status")).await.unwrap_or_default();
            if !status.lines().any(|line| line == format!("Uid:\t{}\t{}\t{}\t{}", self.uid, self.uid, self.uid, self.uid))
                && !status.lines().any(|line| line.starts_with(&format!("Uid:\t{}\t", self.uid)))
            {
                continue;
            }
            let Ok(executable) = tokio::fs::read_link(format!("/proc/{pid}/exe")).await else { continue; };
            let Some(action) = app_action(&rules, &executable) else { continue; };
            let group = match action {
                RuleAction::Proxy => "sshrocket-proxy",
                RuleAction::Direct => "sshrocket-direct",
                RuleAction::Block => "sshrocket-block",
            };
            let _ = tokio::fs::write(format!("/sys/fs/cgroup/{group}/cgroup.procs"), pid.to_string()).await;
        }
        Ok(())
    }

    async fn restore_app_processes(&self) {
        for group in ["sshrocket-proxy", "sshrocket-direct", "sshrocket-block"] {
            let Ok(pids) = tokio::fs::read_to_string(format!("/sys/fs/cgroup/{group}/cgroup.procs")).await else {
                continue;
            };
            for pid in pids.lines() {
                let _ = tokio::fs::write("/sys/fs/cgroup/cgroup.procs", pid).await;
            }
        }
    }

    async fn install_nftables(&self) -> Result<()> {
        let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
        command("nft", &["add", "table", "inet", NFT_TABLE]).await?;
        command("nft", &["add", "chain", "inet", NFT_TABLE, "output", "{", "type", "route", "hook", "output", "priority", "mangle", ";", "policy", "accept", ";", "}"]).await?;
        command("nft", &["add", "chain", "inet", NFT_TABLE, "dns_output", "{", "type", "nat", "hook", "output", "priority", "dstnat", ";", "policy", "accept", ";", "}"]).await?;

        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "skuid", "!=", &self.uid.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "127.0.0.0/8", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "224.0.0.0/4", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "255.255.255.255", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip6", "daddr", "::1", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip6", "daddr", "fe80::/10", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip6", "daddr", "ff00::/8", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "tcp", "dport", &self.socks_port.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "tcp", "dport", &self.dns_port.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "223.5.5.5", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "114.114.114.114", "return"]).await?;
        for address in &self.ssh_addresses {
            let family = if address.is_ipv4() { "ip" } else { "ip6" };
            let address = address.to_string();
            command(
                "nft",
                &[
                    "add",
                    "rule",
                    "inet",
                    NFT_TABLE,
                    "output",
                    family,
                    "daddr",
                    &address,
                    "tcp",
                    "dport",
                    &self.ssh_port.to_string(),
                    "return",
                ],
            )
            .await?;
        }
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "198.18.0.0/15", "meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "dns_output", "meta", "skuid", "0", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "dns_output", "udp", "dport", "53", "redirect", "to", &format!(":{DNS_LISTEN_PORT}")]).await?;

        self.install_ip_rules(&self.config.settings.custom_overrides).await?;
        self.install_cgroup_rules().await?;
        self.install_domain_sets().await?;
        self.install_ip_rules(&self.config.settings.ip_rules).await?;
        self.install_ip_rules(&self.config.settings.imported_ip_rules).await?;

        match self.config.settings.default_policy {
            RuleAction::Proxy => {
                command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "udp", "dport", "443", "reject"]).await?;
                command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "l4proto", "tcp", "meta", "mark", "set", MARK]).await?;
            }
            RuleAction::Block => command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "l4proto", "tcp", "reject"]).await?,
            RuleAction::Direct => {}
        }
        Ok(())
    }

    async fn install_domain_sets(&self) -> Result<()> {
        for (name, address_type) in [
            ("domain_proxy4", "ipv4_addr"),
            ("domain_direct4", "ipv4_addr"),
            ("domain_block4", "ipv4_addr"),
            ("domain_proxy6", "ipv6_addr"),
            ("domain_direct6", "ipv6_addr"),
            ("domain_block6", "ipv6_addr"),
        ] {
            command("nft", &["add", "set", "inet", NFT_TABLE, name, "{", "type", address_type, ";", "flags", "timeout", ";", "}"]).await?;
        }

        for family in ["ip", "ip6"] {
            let suffix = if family == "ip" { "4" } else { "6" };
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_block{suffix}"), "reject"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_direct{suffix}"), "return"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_proxy{suffix}"), "udp", "dport", "443", "reject"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_proxy{suffix}"), "meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]).await?;
        }
        Ok(())
    }

    async fn install_cgroup_rules(&self) -> Result<()> {
        for (group, action) in [
            ("sshrocket-block", RuleAction::Block),
            ("sshrocket-direct", RuleAction::Direct),
            ("sshrocket-proxy", RuleAction::Proxy),
        ] {
            let mut args = vec!["add", "rule", "inet", NFT_TABLE, "output", "socket", "cgroupv2", "level", "1", group];
            match action {
                RuleAction::Block => {
                    args.push("reject");
                    command("nft", &args).await?;
                }
                RuleAction::Direct => {
                    args.push("return");
                    command("nft", &args).await?;
                }
                RuleAction::Proxy => {
                    let mut reject_quic = args.clone();
                    reject_quic.extend(["udp", "dport", "443", "reject"]);
                    command("nft", &reject_quic).await?;
                    args.extend(["meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]);
                    command("nft", &args).await?;
                }
            }
        }
        Ok(())
    }

    async fn install_ip_rules(&self, rules: &[ssh_rocket_core::IpRule]) -> Result<()> {
        for rule in rules {
            let family = match rule.network {
                IpNet::V4(_) => "ip",
                IpNet::V6(_) => "ip6",
            };
            let network = rule.network.to_string();
            let mut args = vec!["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", network.as_str()];
            match rule.action {
                RuleAction::Block => {
                    args.push("reject");
                    command("nft", &args).await?;
                }
                RuleAction::Direct => {
                    args.push("return");
                    command("nft", &args).await?;
                }
                RuleAction::Proxy => {
                    let mut reject_quic = args.clone();
                    reject_quic.extend(["udp", "dport", "443", "reject"]);
                    command("nft", &reject_quic).await?;
                    args.extend(["meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]);
                    command("nft", &args).await?;
                }
            }
        }
        Ok(())
    }
}

fn app_action(rules: &HashMap<PathBuf, RuleAction>, executable: &Path) -> Option<RuleAction> {
    if let Some(action) = rules.get(executable) {
        return Some(*action);
    }
    let executable_name = executable.file_name()?;
    rules
        .iter()
        .find(|(rule, _)| rule.file_name() == Some(executable_name))
        .map(|(_, action)| *action)
}

async fn run_dns_router(
    socket: UdpSocket,
    remote_dns_port: u16,
    routing_engine: RoutingEngine,
    ipv6_enabled: bool,
    shutdown: CancellationToken,
) -> Result<()> {
    let socket = Arc::new(socket);
    let routing_engine = Arc::new(routing_engine);
    let mut buffer = [0_u8; 4096];
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            received = socket.recv_from(&mut buffer) => {
                let (size, peer) = received?;
                let packet = buffer[..size].to_vec();
                let socket = socket.clone();
                let routing_engine = routing_engine.clone();
                tokio::spawn(async move {
                    handle_dns_query(socket, peer, packet, remote_dns_port, routing_engine, ipv6_enabled).await;
                });
            }
        }
    }
}

async fn handle_dns_query(
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    packet: Vec<u8>,
    _remote_dns_port: u16,
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

    let resolve_result = if decision.action == RuleAction::Direct {
        forward_dns_local(&packet).await
    } else {
        forward_dns_virtual(&packet).await
    };

    let response = match resolve_result {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("[error] DNS query failed for {domain} (action: {:?}, source: {:?}): {err}", decision.action, decision.source);
            let servfail = servfail_response(&packet);
            let _ = socket.send_to(&servfail, peer).await;
            return;
        }
    };

    let _ = socket.send_to(&response, peer).await;

    let addresses = parse_dns_addresses(&response);
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

    if !addresses.is_empty() {
        update_domain_addresses(&addresses, decision.action).await;
    }
}

async fn forward_dns_local(packet: &[u8]) -> Result<Vec<u8>> {
    match forward_dns_over_udp("223.5.5.5:53", packet, Duration::from_millis(1500)).await {
        Ok(resp) => Ok(resp),
        Err(_) => forward_dns_over_udp("114.114.114.114:53", packet, Duration::from_secs(3)).await,
    }
}

async fn forward_dns_virtual(packet: &[u8]) -> Result<Vec<u8>> {
    forward_dns_over_udp("10.0.0.1:53", packet, Duration::from_millis(1500)).await
}

async fn forward_dns_over_udp(server: &str, packet: &[u8], timeout_dur: Duration) -> Result<Vec<u8>> {
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

fn parse_dns_query(packet: &[u8]) -> Option<(String, u16)> {
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

fn nodata_response(packet: &[u8]) -> Vec<u8> {
    let mut response = packet.to_vec();
    if response.len() >= 12 {
        response[2] = 0x81 | (response[2] & 0x01);
        response[3] = 0x80;
        response[6..12].fill(0);
    }
    response
}

fn servfail_response(packet: &[u8]) -> Vec<u8> {
    let mut response = packet.to_vec();
    if response.len() >= 12 {
        response[2] = 0x81 | (response[2] & 0x01);
        response[3] = 0x82;
        response[6..12].fill(0);
    }
    response
}

fn nxdomain_response(packet: &[u8]) -> Vec<u8> {
    let mut response = packet.to_vec();
    if response.len() >= 12 {
        response[2] = 0x81 | (response[2] & 0x01);
        response[3] = 0x83;
        response[6..12].fill(0);
    }
    response
}

fn parse_dns_addresses(packet: &[u8]) -> Vec<IpAddr> {
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

fn skip_dns_name(packet: &[u8], mut offset: usize) -> Option<usize> {
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

async fn update_domain_addresses(addresses: &[IpAddr], action: RuleAction) {
    if addresses.is_empty() {
        return;
    }
    let target_cat = match action {
        RuleAction::Proxy => "proxy",
        RuleAction::Direct => "direct",
        RuleAction::Block => "block",
    };
    let mut batch = String::new();
    for address in addresses {
        let suffix = if address.is_ipv4() { "4" } else { "6" };
        let value = address.to_string();
        batch.push_str(&format!("add element inet {NFT_TABLE} domain_{target_cat}{suffix} {{ {value} timeout 300s }}\n"));
    }
    if let Ok(mut child) = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(batch.as_bytes()).await;
        }
        if let Ok(output) = child.wait_with_output().await {
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.contains("No such file or directory") {
                    eprintln!("[error] [nft] update_domain_addresses failed: {err}");
                }
            }
        }
    }
}

async fn wait_for_interface() -> Result<()> {
    for _ in 0..100 {
        if Path::new(&format!("/sys/class/net/{TUN_NAME}")).exists() {
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }
    bail!("TUN interface {TUN_NAME} was not created")
}

async fn command(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        bail!("{} {} failed: {}", program, args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

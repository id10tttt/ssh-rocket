use anyhow::{Context, Result, bail};
use ssh_rocket_core::{AppConfig, GlobalSettings};
use ssh_rocket_runtime::{
    dns_router::run_dns_router,
    ipc::{HelperCommand, HelperEvent},
    system::{
        clean_stale_resources, ensure_persistent_tun, wait_for_interface, SystemState,
        MAX_TUN_RETRIES, TUN_DNS_ADDRESS, TUN_NAME,
    },
    DNS_ROUTER_PORT,
};
use std::{
    env,
    net::IpAddr,
    path::PathBuf,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UdpSocket,
    signal,
    task::JoinHandle,
    time::{interval, sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use tun2proxy::{ArgDns, ArgProxy, Args};

const TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

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
    async fn stop(mut self) {
        self.shutdown.cancel();
        if !self.dns_task.is_finished() {
            if timeout(TASK_SHUTDOWN_TIMEOUT, &mut self.dns_task).await.is_err() {
                eprintln!("[helper] DNS task did not stop in time, aborting it");
                self.dns_task.abort();
                let _ = self.dns_task.await;
            }
        }
        if !self.tun_task.is_finished() {
            if timeout(TASK_SHUTDOWN_TIMEOUT, &mut self.tun_task).await.is_err() {
                eprintln!("[helper] TUN task did not stop in time, aborting it");
                self.tun_task.abort();
                let _ = self.tun_task.await;
            }
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
                        match sync_active_rules(&mut active_session).await {
                            Ok(()) => send_event(&mut stdout, &HelperEvent::RulesSynced).await?,
                            Err(err) => {
                                eprintln!("[helper] failed to sync rules: {err}");
                                send_event(&mut stdout, &HelperEvent::Error { message: err.to_string() }).await?;
                            }
                        }
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
                if let Err(error) = sync_active_rules(&mut active_session).await {
                    eprintln!("[helper] rule synchronization failed: {error}");
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

/// 路由配置改变时重建数据面；仅应用规则改变时只重新分配进程。
async fn sync_active_rules(active_session: &mut Option<ActiveSession>) -> Result<()> {
    let Some(session) = active_session.as_mut() else {
        return Ok(());
    };
    let config_path = session.system.config_path.clone();
    let current_mtime = tokio::fs::metadata(&config_path)
        .await
        .and_then(|m| m.modified())
        .ok();

    // 若配置文件修改时间未变化，说明规则文件未更改，直接复用内存中规则分配进程
    if current_mtime.is_some() && current_mtime == session.system.config_mtime {
        return session.system.assign_apps().await;
    }

    let config: AppConfig = serde_json::from_slice(&tokio::fs::read(&config_path).await?)?;
    session.system.config_mtime = current_mtime;
    if routing_signature(&config.settings)? == routing_signature(&session.system.config.settings)? {
        session.system.config.settings.app_rules = config.settings.app_rules;
        return session.system.assign_apps().await;
    }

    let uid = session.system.uid;
    let socks_port = session.system.socks_port;
    let dns_port = session.system.dns_port;
    let ssh_port = session.system.ssh_port;
    let ssh_addresses = session.system.ssh_addresses.clone();
    eprintln!("[helper] routing rules changed, rebuilding transparent proxy");
    active_session.take().unwrap().stop().await;
    *active_session = Some(start_proxy_session(
        config_path,
        uid,
        socks_port,
        dns_port,
        ssh_port,
        ssh_addresses,
    ).await?);
    Ok(())
}

fn routing_signature(settings: &GlobalSettings) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        settings.default_policy,
        &settings.custom_overrides,
        &settings.domain_rules,
        &settings.imported_domain_rules,
        &settings.ip_rules,
        &settings.imported_ip_rules,
        settings.ipv6,
    ))?)
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
        .dns(ArgDns::Direct)
        .ipv6_enabled(config.settings.ipv6)
        .setup(false);

    // 1. 附加到常驻 TUN，并重试解决 EBUSY
    let mut tun_task = None;
    let mut shutdown = CancellationToken::new();
    let mut last_error = String::new();

    for attempt in 1..=MAX_TUN_RETRIES {
        clean_stale_resources().await;
        sleep(Duration::from_millis(100)).await;
        ensure_persistent_tun().await?;

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
    let dns_socket = match UdpSocket::bind(("0.0.0.0", DNS_ROUTER_PORT)).await {
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

    let resolver_socket = match UdpSocket::bind((TUN_DNS_ADDRESS, 53)).await {
        Ok(socket) => socket,
        Err(error) => {
            shutdown.cancel();
            let _ = tun_task.await;
            system.cleanup().await;
            clean_stale_resources().await;
            return Err(error).context("failed to bind system resolver DNS endpoint");
        }
    };
    if let Err(error) = system.configure_system_resolver().await {
        shutdown.cancel();
        let _ = tun_task.await;
        system.cleanup().await;
        clean_stale_resources().await;
        return Err(error).context("failed to configure system resolver");
    }

    let dns_shutdown = shutdown.clone();
    let routing_engine = ssh_rocket_core::RoutingEngine::new(config.settings.clone());
    let ipv6_enabled = config.settings.ipv6;
    let dns_task = tokio::spawn(async move {
        run_dns_router(
            dns_socket,
            resolver_socket,
            dns_port,
            routing_engine,
            ipv6_enabled,
            dns_shutdown,
        )
        .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_rocket_core::{AppRule, DomainRule, DomainRuleKind, RuleAction};

    #[test]
    fn routing_signature_tracks_routing_changes_only() {
        let mut settings = GlobalSettings::default();
        let original = routing_signature(&settings).unwrap();
        settings.app_rules.push(AppRule {
            executable: "firefox".into(),
            action: RuleAction::Direct,
        });
        assert_eq!(routing_signature(&settings).unwrap(), original);

        settings.domain_rules.push(DomainRule {
            pattern: "example.com".into(),
            action: RuleAction::Proxy,
            kind: DomainRuleKind::DomainSuffix,
        });
        assert_ne!(routing_signature(&settings).unwrap(), original);
    }
}

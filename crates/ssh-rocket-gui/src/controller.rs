use ssh_rocket_core::{AppConfig, PortForwardRule, Profile, RuleImportResult};
use ssh_rocket_runtime::{
    check_dns_health, check_socks_health, ForwardManager, PrivilegedHelperSession, SshSession,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    runtime::Handle,
    sync::{oneshot, Mutex},
};
use uuid::Uuid;

use crate::{
    app_scanner::scan_desktop_apps,
    traffic_tracker::{read_ssh_traffic, ActiveConnectionStat, AppTrafficStat, AppTrafficTracker},
    SOCKS_PORT,
};

pub enum RuntimeEvent {
    Connected,
    Disconnected,
    Status(String),
    Error(String),
    Log(String),
    Speed { upload: u64, download: u64 },
    AppTraffic {
        stats: Vec<AppTrafficStat>,
        conns: Vec<ActiveConnectionStat>,
    },
    RuleImportFailed(String),
    RulesImported {
        result: RuleImportResult,
        source_url: String,
    },
}

#[derive(Clone)]
pub struct RuntimeController {
    stop: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    helper: Arc<Mutex<Option<PrivilegedHelperSession>>>,
    is_running: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    worker: Arc<Mutex<()>>,
    forward_manager: Arc<Mutex<ForwardManager>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Default for RuntimeController {
    fn default() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("无法初始化后台 Tokio 运行时");
        Self {
            stop: Rc::new(RefCell::new(None)),
            helper: Arc::new(Mutex::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            worker: Arc::new(Mutex::new(())),
            forward_manager: Arc::new(Mutex::new(ForwardManager::new())),
            runtime: Arc::new(runtime),
        }
    }
}

impl RuntimeController {
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(stop) = self.stop.borrow_mut().take() {
            let _ = stop.send(());
        }
    }

    /// 在后台清理透明代理与端口转发资源，并在完成后通知 UI。
    pub fn shutdown(&self) -> mpsc::Receiver<()> {
        self.stop();
        let helper = self.helper.clone();
        let forward_manager = self.forward_manager.clone();
        let runtime = self.runtime.clone();
        let (completed_tx, completed_rx) = mpsc::channel();
        thread::spawn(move || {
            runtime.block_on(async {
                let shutdown_helper = async {
                    let mut guard = helper.lock().await;
                    if let Some(mut h) = guard.take() {
                        h.shutdown().await;
                    }
                };
                let shutdown_forwards = async {
                    forward_manager.lock().await.stop_all().await;
                };
                tokio::join!(shutdown_helper, shutdown_forwards);
            });
            let _ = completed_tx.send(());
        });
        completed_rx
    }

    /// 启动配置中所有已启用的端口转发规则。
    pub fn start_enabled_forwards(
        &self,
        config: AppConfig,
        events: mpsc::Sender<RuntimeEvent>,
    ) {
        let forward_manager = self.forward_manager.clone();
        self.runtime.spawn(async move {
            let mut manager = forward_manager.lock().await;
            for rule in config.port_forwards.iter().filter(|rule| rule.enabled) {
                let Some(profile) = config
                    .profiles
                    .iter()
                    .find(|profile| profile.id == rule.profile_id)
                else {
                    let _ = events.send(RuntimeEvent::Log(format!(
                        "[forward] 无法启动 {}：未找到关联的 SSH 节点",
                        rule.name
                    )));
                    continue;
                };
                match manager.start(rule, profile).await {
                    Ok(()) => {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[forward] 已启动端口转发：{}",
                            rule.name
                        )));
                    }
                    Err(error) => {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[forward] 启动 {} 失败：{error}",
                            rule.name
                        )));
                    }
                }
            }
        });
    }

    /// 根据界面操作启动或停止单条端口转发规则。
    pub fn set_forward_enabled(
        &self,
        rule: PortForwardRule,
        profile: Option<Profile>,
        enabled: bool,
        events: mpsc::Sender<RuntimeEvent>,
    ) {
        let forward_manager = self.forward_manager.clone();
        self.runtime.spawn(async move {
            let mut manager = forward_manager.lock().await;
            if enabled {
                let Some(profile) = profile else {
                    let _ = events.send(RuntimeEvent::Log(format!(
                        "[forward] 无法启动 {}：未找到关联的 SSH 节点",
                        rule.name
                    )));
                    return;
                };
                match manager.start(&rule, &profile).await {
                    Ok(()) => {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[forward] 已启动端口转发：{}",
                            rule.name
                        )));
                    }
                    Err(error) => {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[forward] 启动 {} 失败：{error}",
                            rule.name
                        )));
                    }
                }
            } else {
                manager.stop(&rule.id).await;
                let _ = events.send(RuntimeEvent::Log(format!(
                    "[forward] 已停止端口转发：{}",
                    rule.name
                )));
            }
        });
    }

    /// 停止并移除指定端口转发对应的后台会话。
    pub fn stop_forward(&self, rule_id: Uuid, events: mpsc::Sender<RuntimeEvent>) {
        let forward_manager = self.forward_manager.clone();
        self.runtime.spawn(async move {
            forward_manager.lock().await.stop(&rule_id).await;
            let _ = events.send(RuntimeEvent::Log(format!(
                "[forward] 已删除端口转发：{rule_id}"
            )));
        });
    }

    pub fn sync_rules(&self) {
        let helper = self.helper.clone();
        self.runtime.spawn(async move {
            let mut guard = helper.lock().await;
            if let Some(h) = guard.as_mut() {
                let _ = h.sync_rules().await;
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
        let task_generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.is_running.store(true, Ordering::SeqCst);
        let (stop_tx, mut stop_rx) = oneshot::channel();
        *self.stop.borrow_mut() = Some(stop_tx);

        let helper = self.helper.clone();
        let is_running_flag = self.is_running.clone();
        let generation = self.generation.clone();
        let worker = self.worker.clone();
        let runtime_handle = self.runtime.handle().clone();

        self.runtime.spawn(async move {
            let _worker_guard = worker.lock().await;
            if generation.load(Ordering::SeqCst) != task_generation {
                return;
            }
            let _ = events.send(RuntimeEvent::Status("正在连接…".into()));
            let desktop_apps = scan_desktop_apps();
            let user_cancelled = Arc::new(AtomicBool::new(false));
            let mut retry_attempt = 0;
            let mut app_tracker = AppTrafficTracker::default();

            loop {
                if user_cancelled.load(Ordering::SeqCst)
                    || generation.load(Ordering::SeqCst) != task_generation
                {
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
                                spawn_log_reader(
                                    &runtime_handle,
                                    stderr,
                                    "helper",
                                    events.clone(),
                                );
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
                    spawn_log_reader(&runtime_handle, stderr, "ssh", events.clone());
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
                drop(helper_guard);

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

                let mut health_interval = tokio::time::interval(Duration::from_secs(15));
                health_interval.tick().await; // 消耗初始即时触发，连接建立 15 秒后再执行首次存活检测
                let mut health_failures = 0u32;
                const MAX_HEALTH_FAILURES: u32 = 3;

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
                            let _ = events.send(RuntimeEvent::Status("连接断开，正在准备重连…".into()));
                            break;
                        }
                        _ = health_interval.tick() => {
                            let health = async {
                                check_socks_health(ssh.socks_port, Duration::from_secs(5)).await?;
                                check_dns_health(ssh.dns_port, Duration::from_secs(8)).await?;
                                Ok::<(), anyhow::Error>(())
                            }.await;
                            match health {
                                Ok(()) => {
                                    if health_failures > 0 {
                                        let _ = events.send(RuntimeEvent::Log(
                                            "[health] 存活检测已恢复正常".into(),
                                        ));
                                        health_failures = 0;
                                    }
                                }
                                Err(error) => {
                                    health_failures += 1;
                                    let _ = events.send(RuntimeEvent::Log(format!(
                                        "[health] 存活检测未通过 ({health_failures}/{MAX_HEALTH_FAILURES}): {error}"
                                    )));
                                    if health_failures >= MAX_HEALTH_FAILURES {
                                        disconnect_reason = format!(
                                            "连续 {MAX_HEALTH_FAILURES} 次存活检测失败: {error}"
                                        );
                                        let _ = events.send(RuntimeEvent::Log(format!(
                                            "[health] {disconnect_reason}，准备自动重连..."
                                        )));
                                        let _ = events.send(RuntimeEvent::Status("连接异常，正在准备重连…".into()));
                                        break;
                                    }
                                }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {
                            let mut helper_guard = helper.lock().await;
                            let helper_health = match helper_guard.as_mut() {
                                Some(helper) => helper.check_active().await,
                                None => Err(anyhow::anyhow!("特权 Helper 会话不可用")),
                            };
                            if let Err(error) = helper_health {
                                disconnect_reason = format!("透明代理健康检查失败: {error}");
                                let _ = events.send(RuntimeEvent::Log(format!("[helper] {disconnect_reason}")));
                                let _ = events.send(RuntimeEvent::Status("连接异常，正在准备重连…".into()));
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
                            if let Some((stats, conns)) = app_tracker.sample(&desktop_apps, ssh.socks_port).await {
                                let _ = events.send(RuntimeEvent::AppTraffic { stats, conns });
                            }
                        }
                    }
                }

                let mut helper_guard = helper.lock().await;
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

            if generation.load(Ordering::SeqCst) == task_generation {
                is_running_flag.store(false, Ordering::SeqCst);
            }
        });
    }
}

pub fn spawn_log_reader<R>(
    runtime: &Handle,
    reader: R,
    source: &'static str,
    events: mpsc::Sender<RuntimeEvent>,
)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    runtime.spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = events.send(RuntimeEvent::Log(format!("[{source}] {line}")));
        }
    });
}

pub fn helper_path() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::RuntimeController;
    use std::time::Duration;

    #[test]
    fn shutdown_completes_without_active_sessions() {
        let controller = RuntimeController::default();
        let completed = controller.shutdown();

        assert!(completed.recv_timeout(Duration::from_secs(2)).is_ok());
    }
}

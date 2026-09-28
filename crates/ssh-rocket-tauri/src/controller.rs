use crate::{
    scanner::scan_desktop_apps,
    traffic::{read_ssh_traffic, AppTrafficTracker},
    types::SpeedDto,
};
use ssh_rocket_core::{AppConfig, Profile};
use ssh_rocket_runtime::{check_dns_health, check_socks_health, PrivilegedHelperSession, SshSession};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{oneshot, Mutex},
};

pub const SOCKS_PORT: u16 = 17880;

#[derive(Clone)]
pub struct RuntimeController {
    stop: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    helper: Arc<Mutex<Option<PrivilegedHelperSession>>>,
    is_running: Arc<AtomicBool>,
    status_text: Arc<Mutex<String>>,
    generation: Arc<AtomicU64>,
    worker: Arc<Mutex<()>>,
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self {
            stop: Arc::new(Mutex::new(None)),
            helper: Arc::new(Mutex::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            status_text: Arc::new(Mutex::new("未连接".into())),
            generation: Arc::new(AtomicU64::new(0)),
            worker: Arc::new(Mutex::new(())),
        }
    }
}

impl RuntimeController {
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub async fn get_status_text(&self) -> String {
        self.status_text.lock().await.clone()
    }

    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.is_running.store(false, Ordering::SeqCst);
        *self.status_text.lock().await = "未连接".into();
        let mut stop_guard = self.stop.lock().await;
        if let Some(stop) = stop_guard.take() {
            let _ = stop.send(());
        }
    }

    pub async fn shutdown(&self) {
        self.stop().await;
        let mut guard = self.helper.lock().await;
        if let Some(mut h) = guard.take() {
            h.shutdown().await;
        }
    }

    pub async fn sync_rules(&self) {
        let mut guard = self.helper.lock().await;
        if let Some(h) = guard.as_mut() {
            let _ = h.sync_rules().await;
        }
    }

    pub async fn start(&self, app_handle: AppHandle, profile: Profile, config: AppConfig) {
        self.stop().await;
        let task_generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.is_running.store(true, Ordering::SeqCst);
        *self.status_text.lock().await = "正在连接…".into();
        let (stop_tx, mut stop_rx) = oneshot::channel();
        *self.stop.lock().await = Some(stop_tx);

        let helper = self.helper.clone();
        let is_running_flag = self.is_running.clone();
        let status_text = self.status_text.clone();
        let generation = self.generation.clone();
        let worker = self.worker.clone();
        let config_path = AppConfig::path().unwrap_or_else(|_| PathBuf::from("config.json"));

        tokio::spawn(async move {
            let _worker_guard = worker.lock().await;
            if generation.load(Ordering::SeqCst) != task_generation {
                return;
            }

            let _ = app_handle.emit("status-changed", "正在连接…");
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
                    let text = format!("正在重连 ({retry_attempt})…");
                    *status_text.lock().await = text.clone();
                    let _ = app_handle.emit("status-changed", &text);
                    let _ = app_handle.emit("log", format!("[reconnect] 正在重新建立 SSH 连接 (第 {retry_attempt} 次)..."));
                }

                // 1. 检查并重置 helper
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
                                    Err(err) => {
                                        let _ = app_handle.emit("log", format!("[helper] 重置会话失败: {err}"));
                                        false
                                    }
                                },
                                Err(err) => {
                                    let _ = app_handle.emit("log", format!("[helper] Helper 无响应: {err}"));
                                    false
                                }
                            }
                        }
                    }
                    None => false,
                };

                if !helper_usable {
                    if let Some(mut stale) = helper_guard.take() {
                        stale.terminate().await;
                    }
                    let _ = app_handle.emit("log", "[helper] 请求特权 Helper 授权...".to_string());
                    let _ = app_handle.emit("status-changed", "正在授权 Helper…");
                    match PrivilegedHelperSession::ensure_started(&helper_path()).await {
                        Ok((h, stderr)) => {
                            if let Some(stderr) = stderr {
                                spawn_log_emitter(stderr, "helper", app_handle.clone());
                            }
                            *helper_guard = Some(h);
                            let _ = app_handle.emit("log", "[helper] 特权 Helper 认证成功并就绪".to_string());
                        }
                        Err(err) => {
                            if user_cancelled.load(Ordering::SeqCst) {
                                break;
                            }
                            let msg = format!("Helper 授权失败: {err}");
                            let _ = app_handle.emit("error", &msg);
                            let _ = app_handle.emit("log", format!("[helper] {msg}"));
                            break;
                        }
                    }
                }

                // 2. 建立 SSH 会话
                let mut ssh = match SshSession::start(
                    &profile,
                    SOCKS_PORT,
                    config.settings.dns_server,
                )
                .await
                {
                    Ok(session) => session,
                    Err(err) => {
                        drop(helper_guard);
                        if user_cancelled.load(Ordering::SeqCst) {
                            break;
                        }
                        retry_attempt += 1;
                        let _ = app_handle.emit("log", format!("[reconnect] SSH 连接失败: {err}。1秒后重试 (第 {retry_attempt} 次)..."));
                        let _ = app_handle.emit("status-changed", "1秒后重试…");
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
                    spawn_log_emitter(stderr, "ssh", app_handle.clone());
                }

                #[cfg(target_os = "linux")]
                let uid = unsafe { libc::getuid() };
                #[cfg(not(target_os = "linux"))]
                let uid = 501;

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
                    if let Some(mut failed) = helper_guard.take() {
                        failed.terminate().await;
                    }
                    if user_cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    retry_attempt += 1;
                    let _ = app_handle.emit("log", format!("[reconnect] 透明代理启动失败: {err}。1秒后重试 (第 {retry_attempt} 次)..."));
                    let _ = app_handle.emit("status-changed", "1秒后重试…");
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
                    let _ = app_handle.emit("log", "[reconnect] 连接已恢复".to_string());
                }
                retry_attempt = 0;
                *status_text.lock().await = "已连接".into();
                let _ = app_handle.emit("connected", ());
                let _ = app_handle.emit("status-changed", "已连接");

                let mut traffic_interval = tokio::time::interval(Duration::from_secs(1));
                let mut previous_traffic = None;
                let mut disconnect_reason = String::new();

                let mut health_interval = tokio::time::interval(Duration::from_secs(15));
                health_interval.tick().await;
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
                                Ok(st) => format!("SSH 进程退出，状态码: {st}"),
                                Err(err) => err.to_string(),
                            };
                            let _ = app_handle.emit("log", format!("[reconnect] {disconnect_reason}"));
                            let _ = app_handle.emit("status-changed", "连接断开，正在准备重连…");
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
                                        let _ = app_handle.emit("log", "[health] 存活检测已恢复正常".to_string());
                                        health_failures = 0;
                                    }
                                }
                                Err(err) => {
                                    health_failures += 1;
                                    let _ = app_handle.emit("log", format!("[health] 存活检测未通过 ({health_failures}/{MAX_HEALTH_FAILURES}): {err}"));
                                    if health_failures >= MAX_HEALTH_FAILURES {
                                        disconnect_reason = format!("连续 {MAX_HEALTH_FAILURES} 次存活检测失败: {err}");
                                        let _ = app_handle.emit("log", format!("[health] {disconnect_reason}，准备自动重连..."));
                                        let _ = app_handle.emit("status-changed", "连接异常，正在准备重连…");
                                        break;
                                    }
                                }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {
                            let mut helper_guard = helper.lock().await;
                            let helper_health = match helper_guard.as_mut() {
                                Some(h) => h.check_active().await,
                                None => Err(anyhow::anyhow!("特权 Helper 会话不可用")),
                            };
                            if let Err(err) = helper_health {
                                disconnect_reason = format!("透明代理健康检查失败: {err}");
                                let _ = app_handle.emit("log", format!("[helper] {disconnect_reason}"));
                                let _ = app_handle.emit("status-changed", "连接异常，正在准备重连…");
                                break;
                            }
                        }
                        _ = traffic_interval.tick() => {
                            if let Some((sent, received)) = read_ssh_traffic(&profile.host).await {
                                let (upload, download) = previous_traffic
                                    .map(|(old_s, old_r)| {
                                        (sent.saturating_sub(old_s), received.saturating_sub(old_r))
                                    })
                                    .unwrap_or((0, 0));
                                previous_traffic = Some((sent, received));
                                let speed = SpeedDto { upload, download };
                                let _ = app_handle.emit("speed", &speed);
                            }
                            if let Some((stats, conns)) = app_tracker.sample(&desktop_apps, ssh.socks_port).await {
                                let _ = app_handle.emit("app-traffic", &stats);
                                let _ = app_handle.emit("active-connections", &conns);
                            }
                        }
                    }
                }

                let mut helper_guard = helper.lock().await;
                if let Some(h) = helper_guard.as_mut() {
                    let _ = h.stop().await;
                }
                drop(helper_guard);
                let _ = ssh.stop().await;
                let _ = app_handle.emit("speed", &SpeedDto { upload: 0, download: 0 });
                app_tracker.clear();

                if user_cancelled.load(Ordering::SeqCst) {
                    let _ = app_handle.emit("disconnected", ());
                    break;
                }

                retry_attempt += 1;
                *status_text.lock().await = "连接断开，正在准备重连…".into();
                let _ = app_handle.emit("status-changed", "连接断开，正在准备重连…");
                let _ = app_handle.emit("log", format!("[reconnect] 连接已断开 ({disconnect_reason})。1秒后尝试重连 (第 {retry_attempt} 次)..."));
                tokio::select! {
                    _ = &mut stop_rx => {
                        user_cancelled.store(true, Ordering::SeqCst);
                        let _ = app_handle.emit("disconnected", ());
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                }
            }

            if generation.load(Ordering::SeqCst) == task_generation {
                is_running_flag.store(false, Ordering::SeqCst);
                *status_text.lock().await = "未连接".into();
                let _ = app_handle.emit("status-changed", "未连接");
            }
        });
    }
}

pub fn spawn_log_emitter<R>(reader: R, source: &'static str, app: AppHandle)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app.emit("log", format!("[{source}] {line}"));
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

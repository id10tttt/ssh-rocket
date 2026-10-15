use anyhow::{bail, Context, Result};
use log::info;
use ssh_rocket_core::{ForwardType, PortForwardRule, Profile};
use std::{collections::HashMap, net::TcpListener, process::Stdio, time::Duration};
use tokio::{
    process::{Child, Command},
    sync::mpsc::UnboundedSender,
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INITIAL_RETRY_DELAY_SECS: u64 = 1;
const MAX_RETRY_DELAY_SECS: u64 = 30;

pub struct ForwardSession {
    child: Child,
    pub rule_id: Uuid,
    pub forward_type: ForwardType,
    pub local_port: u16,
}

impl ForwardSession {
    /// 启动单个端口转发后台会话。
    ///
    /// 对于本地转发 (-L)，先做本地端口可用性快速探测，拉起进程后轮询本地端口直至监听就绪。
    /// 对于远程转发 (-R)，依赖 OpenSSH 的 ExitOnForwardFailure 检查远端监听结果。
    pub async fn start(rule: &PortForwardRule, profile: &Profile) -> Result<Self> {
        if profile.host.trim().is_empty() {
            bail!("SSH 节点主机地址为空");
        }
        if rule.local_port == 0 || rule.remote_port == 0 {
            bail!("端口号不能为 0");
        }

        // 本地转发启动前先检查本地端口是否被占用
        if rule.forward_type == ForwardType::Local {
            let bind_addr = format!("{}:{}", rule.local_host.trim(), rule.local_port);
            if TcpListener::bind(&bind_addr).is_err() {
                bail!("本地端口 {} 已被其他进程占用", rule.local_port);
            }
        }

        let forward_arg = match rule.forward_type {
            ForwardType::Local => {
                format!(
                    "{}:{}:{}:{}",
                    rule.local_host.trim(),
                    rule.local_port,
                    rule.remote_host.trim(),
                    rule.remote_port
                )
            }
            ForwardType::Remote => {
                format!(
                    "{}:{}:{}:{}",
                    rule.remote_host.trim(),
                    rule.remote_port,
                    rule.local_host.trim(),
                    rule.local_port
                )
            }
        };

        let use_sshpass = profile.auth_type == ssh_rocket_core::AuthType::Password && profile.password.is_some();
        let mut command = if use_sshpass {
            let mut cmd = Command::new("sshpass");
            if let Some(password) = &profile.password {
                cmd.arg("-p").arg(password);
            }
            cmd.arg("ssh");
            cmd
        } else {
            Command::new("ssh")
        };

        command
            .arg("-N")
            .arg("-T")
            .arg("-o").arg("ExitOnForwardFailure=yes")
            .arg("-o").arg("ServerAliveInterval=15")
            .arg("-o").arg("ServerAliveCountMax=3")
            .arg("-o").arg("StrictHostKeyChecking=accept-new")
            .arg("-p").arg(profile.port.to_string());

        match rule.forward_type {
            ForwardType::Local => {
                command.arg("-L").arg(forward_arg);
            }
            ForwardType::Remote => {
                command.arg("-R").arg(forward_arg);
            }
        }

        if !profile.username.trim().is_empty() {
            command.arg("-l").arg(profile.username.trim());
        }
        if profile.auth_type == ssh_rocket_core::AuthType::Key {
            if let Some(identity_file) = &profile.identity_file {
                command.arg("-i").arg(identity_file);
            }
        }

        #[cfg(target_os = "linux")]
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }

        command
            .arg(profile.host.trim())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().context("启动 SSH 端口转发进程失败")?;

        // 若为本地转发，验证本地端口是否就绪
        if rule.forward_type == ForwardType::Local {
            let port = rule.local_port;
            let host = rule.local_host.clone();
            let check_ready = async {
                for _ in 0..50 {
                    if let Ok(Some(status)) = child.try_wait() {
                        bail!("SSH 端口转发提前退出: {}", status);
                    }
                    if tokio::net::TcpStream::connect((host.as_str(), port)).await.is_ok() {
                        return Ok(());
                    }
                    sleep(Duration::from_millis(100)).await;
                }
                bail!("等待本地端口 {} 监听超时", port);
            };

            if let Err(err) = check_ready.await {
                let _ = child.kill().await;
                return Err(err);
            }
        } else {
            // 远程转发：等待 500ms 观察是否有即刻退出（如权限拒绝或远端端口冲突）
            sleep(Duration::from_millis(500)).await;
            if let Ok(Some(status)) = child.try_wait() {
                bail!("SSH 远程端口转发建立失败退出: {}", status);
            }
        }

        Ok(Self {
            child,
            rule_id: rule.id,
            forward_type: rule.forward_type,
            local_port: rule.local_port,
        })
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            _ => false,
        }
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.child.id().is_some() {
            let _ = self.child.kill().await;
        }
        Ok(())
    }

    async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.child
            .wait()
            .await
            .context("等待 SSH 端口转发进程退出失败")
    }
}

#[derive(Debug)]
pub enum ForwardStart {
    Established,
    Retrying { error: String, delay_secs: u64 },
}

struct ForwardTask {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl ForwardTask {
    async fn stop(self) {
        self.cancel.cancel();
        let _ = self.handle.await;
    }
}

fn next_retry_delay(current: u64) -> u64 {
    current.saturating_mul(2).min(MAX_RETRY_DELAY_SECS)
}

fn send_event(events: &UnboundedSender<String>, message: String) {
    let _ = events.send(message);
}

/// 持续监督单条端口转发；异常退出后独立重建，不依赖代理会话状态。
async fn supervise_forward(
    rule: PortForwardRule,
    profile: Profile,
    mut session: Option<ForwardSession>,
    cancel: CancellationToken,
    events: UnboundedSender<String>,
) {
    let mut retry_delay_secs = INITIAL_RETRY_DELAY_SECS;

    loop {
        if let Some(active_session) = session.as_mut() {
            let exit_result = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = active_session.stop().await;
                    return;
                }
                result = active_session.wait() => result,
            };

            let reason = match exit_result {
                Ok(status) => format!("进程退出：{status}"),
                Err(error) => error.to_string(),
            };
            send_event(
                &events,
                format!(
                    "[forward] {} 已断开（{}），{} 秒后重试",
                    rule.name, reason, retry_delay_secs
                ),
            );
            session = None;
        }

        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = sleep(Duration::from_secs(retry_delay_secs)) => {}
        }

        let start_result = tokio::select! {
            _ = cancel.cancelled() => return,
            result = ForwardSession::start(&rule, &profile) => result,
        };

        match start_result {
            Ok(new_session) => {
                session = Some(new_session);
                retry_delay_secs = INITIAL_RETRY_DELAY_SECS;
                send_event(
                    &events,
                    format!("[forward] 已恢复端口转发：{}", rule.name),
                );
            }
            Err(error) => {
                retry_delay_secs = next_retry_delay(retry_delay_secs);
                send_event(
                    &events,
                    format!(
                        "[forward] 重启 {} 失败：{}；{} 秒后重试",
                        rule.name, error, retry_delay_secs
                    ),
                );
            }
        }
    }
}

/// 集中管理所有正在运行的端口转发会话。
#[derive(Default)]
pub struct ForwardManager {
    tasks: HashMap<Uuid, ForwardTask>,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(
        &mut self,
        rule: &PortForwardRule,
        profile: &Profile,
        events: UnboundedSender<String>,
    ) -> ForwardStart {
        self.stop(&rule.id).await;

        let (session, result) = match ForwardSession::start(rule, profile).await {
            Ok(session) => {
                info!("端口转发已建立: {} (ID: {})", rule.name, rule.id);
                (Some(session), ForwardStart::Established)
            }
            Err(error) => (
                None,
                ForwardStart::Retrying {
                    error: error.to_string(),
                    delay_secs: INITIAL_RETRY_DELAY_SECS,
                },
            ),
        };

        let cancel = CancellationToken::new();
        let handle = tokio::spawn(supervise_forward(
            rule.clone(),
            profile.clone(),
            session,
            cancel.clone(),
            events,
        ));
        self.tasks.insert(rule.id, ForwardTask { cancel, handle });
        result
    }

    pub async fn stop(&mut self, rule_id: &Uuid) {
        if let Some(task) = self.tasks.remove(rule_id) {
            task.stop().await;
            info!("端口转发已停止 (ID: {})", rule_id);
        }
    }

    pub async fn stop_all(&mut self) {
        for (id, task) in self.tasks.drain() {
            task.stop().await;
            info!("端口转发已清理停止 (ID: {})", id);
        }
    }

    pub fn is_running(&self, rule_id: &Uuid) -> bool {
        self.tasks
            .get(rule_id)
            .is_some_and(|task| !task.handle.is_finished())
    }

    pub fn active_rule_ids(&self) -> Vec<Uuid> {
        self.tasks
            .iter()
            .filter_map(|(id, task)| (!task.handle.is_finished()).then_some(*id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{next_retry_delay, MAX_RETRY_DELAY_SECS};

    #[test]
    fn retry_delay_uses_exponential_backoff_with_cap() {
        assert_eq!(next_retry_delay(1), 2);
        assert_eq!(next_retry_delay(16), 30);
        assert_eq!(next_retry_delay(MAX_RETRY_DELAY_SECS), MAX_RETRY_DELAY_SECS);
    }
}

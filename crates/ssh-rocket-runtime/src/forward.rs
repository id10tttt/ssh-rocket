use anyhow::{bail, Context, Result};
use log::info;
use ssh_rocket_core::{ForwardType, PortForwardRule, Profile};
use std::{collections::HashMap, net::TcpListener, process::Stdio, time::Duration};
use tokio::{
    process::{Child, Command},
    time::sleep,
};
use uuid::Uuid;

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
}

/// 集中管理所有正在运行的端口转发会话。
#[derive(Default)]
pub struct ForwardManager {
    sessions: HashMap<Uuid, ForwardSession>,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(&mut self, rule: &PortForwardRule, profile: &Profile) -> Result<()> {
        self.stop(&rule.id).await;
        let session = ForwardSession::start(rule, profile).await?;
        self.sessions.insert(rule.id, session);
        info!("端口转发已建立: {} (ID: {})", rule.name, rule.id);
        Ok(())
    }

    pub async fn stop(&mut self, rule_id: &Uuid) {
        if let Some(mut session) = self.sessions.remove(rule_id) {
            let _ = session.stop().await;
            info!("端口转发已停止 (ID: {})", rule_id);
        }
    }

    pub async fn stop_all(&mut self) {
        for (id, mut session) in self.sessions.drain() {
            let _ = session.stop().await;
            info!("端口转发已清理停止 (ID: {})", id);
        }
    }

    pub fn is_running(&mut self, rule_id: &Uuid) -> bool {
        if let Some(session) = self.sessions.get_mut(rule_id) {
            if session.is_alive() {
                return true;
            } else {
                self.sessions.remove(rule_id);
            }
        }
        false
    }

    pub fn active_rule_ids(&mut self) -> Vec<Uuid> {
        let mut dead = Vec::new();
        for (id, session) in &mut self.sessions {
            if !session.is_alive() {
                dead.push(*id);
            }
        }
        for id in dead {
            self.sessions.remove(&id);
        }
        self.sessions.keys().copied().collect()
    }
}

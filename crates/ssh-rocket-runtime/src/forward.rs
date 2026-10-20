use anyhow::{bail, Context, Result};
use log::info;
use ssh_rocket_core::{ForwardType, PortForwardRule, Profile};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc::UnboundedSender, oneshot},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INITIAL_RETRY_DELAY_SECS: u64 = 1;
const MAX_RETRY_DELAY_SECS: u64 = 30;
const START_TIMEOUT: Duration = Duration::from_secs(20);

type PrepareForward = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

#[derive(Clone)]
struct ForwardOptions {
    ssh_program: PathBuf,
    start_timeout: Duration,
    retry_interval: Duration,
}

impl Default for ForwardOptions {
    fn default() -> Self {
        Self {
            ssh_program: PathBuf::from("ssh"),
            start_timeout: START_TIMEOUT,
            retry_interval: Duration::from_secs(INITIAL_RETRY_DELAY_SECS),
        }
    }
}

pub struct ForwardSession {
    child: Child,
    stderr_reader: Option<JoinHandle<()>>,
    pub rule_id: Uuid,
    pub forward_type: ForwardType,
    pub local_port: u16,
}

impl Drop for ForwardSession {
    fn drop(&mut self) {
        if let Some(reader) = &self.stderr_reader {
            reader.abort();
        }
    }
}

impl ForwardSession {
    /// 启动单个端口转发后台会话。
    ///
    /// 本地转发等待进入会话循环，远程转发等待服务端确认监听成功。
    pub async fn start(rule: &PortForwardRule, profile: &Profile) -> Result<Self> {
        let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
        Self::start_with_options(rule, profile, events, &ForwardOptions::default()).await
    }

    /// 为连接、认证和代理解析设置总超时，取消时释放子进程与日志任务。
    async fn start_with_options(
        rule: &PortForwardRule,
        profile: &Profile,
        events: UnboundedSender<ForwardEvent>,
        options: &ForwardOptions,
    ) -> Result<Self> {
        timeout(options.start_timeout, Self::start_inner(rule, profile, events, options))
            .await.context("等待 SSH 认证及转发监听就绪超时")?
    }

    async fn start_inner(
        rule: &PortForwardRule,
        profile: &Profile,
        events: UnboundedSender<ForwardEvent>,
        options: &ForwardOptions,
    ) -> Result<Self> {
        if profile.host.trim().is_empty() {
            bail!("SSH 节点主机地址为空");
        }
        if rule.local_port == 0 || rule.remote_port == 0 {
            bail!("端口号不能为 0");
        }

        // 本地转发启动前先检查本地端口是否被占用
        if rule.forward_type == ForwardType::Local {
            let bind_addr = format!("{}:{}", rule.local_host.trim(), rule.local_port);
            tokio::net::TcpListener::bind(&bind_addr).await
                .with_context(|| format!("本地监听地址 {bind_addr} 不可用"))?;
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
            cmd.arg(&options.ssh_program);
            cmd
        } else {
            Command::new(&options.ssh_program)
        };

        command
            .arg("-v")
            .arg("-N")
            .arg("-T")
            // 日志用于确认监听就绪，禁止连接复用和派生后台进程以确保监督实际会话。
            .env("LC_ALL", "C")
            .arg("-o").arg("LogLevel=DEBUG1")
            .arg("-o").arg("ControlMaster=no")
            .arg("-o").arg("ControlPath=none")
            .arg("-o").arg("ForkAfterAuthentication=no")
            .arg("-o").arg("ExitOnForwardFailure=yes")
            .arg("-o").arg("ConnectTimeout=10")
            .arg("-o").arg("ConnectionAttempts=1")
            .arg("-o").arg("NumberOfPasswordPrompts=1")
            .arg("-o").arg("ServerAliveInterval=15")
            .arg("-o").arg("ServerAliveCountMax=3")
            .arg("-o").arg("StrictHostKeyChecking=accept-new")
            .arg("-p").arg(profile.port.to_string());

        if profile.auth_type == ssh_rocket_core::AuthType::Key {
            command.arg("-o").arg("BatchMode=yes");
        }

        if rule.use_proxy {
            let executable = std::env::current_exe().context("无法定位 SSH Rocket 可执行文件")?;
            let (target_host, target_port) = crate::ssh::effective_server(profile).await;
            let proxy_command = format!(
                "{} proxy-connect {} {}",
                shell_quote_path(&executable),
                shell_quote(&target_host),
                target_port
            );
            command.arg("-o").arg(format!("ProxyCommand={proxy_command}"));
        }

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

        let stderr = child.stderr.take().context("无法读取 SSH 错误输出")?;
        let (ready_tx, ready_rx) = oneshot::channel();
        let rule_id = rule.id;
        let forward_type = rule.forward_type;
        let expected_remote_reply = format!(
            "debug1: remote forward success for: listen {}:{}, connect {}:{}",
            rule.remote_host.trim(), rule.remote_port, rule.local_host.trim(), rule.local_port
        );
        let stderr_reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut ready_tx = Some(ready_tx);
            while let Ok(Some(line)) = lines.next_line().await {
                if forwarding_ready(&line, forward_type, &expected_remote_reply) {
                    if let Some(ready_tx) = ready_tx.take() {
                        let _ = ready_tx.send(());
                    }
                }
                // 始终排空输出，仅将诊断信息写入业务日志，避免管道写满阻塞 SSH。
                if !line.starts_with("debug1:") && !line.starts_with("Authenticated to ") {
                    send_log(&events, format!("[forward] {rule_id} SSH: {line}"));
                }
            }
        });
        let mut session = Self {
            child,
            stderr_reader: Some(stderr_reader),
            rule_id: rule.id,
            forward_type: rule.forward_type,
            local_port: rule.local_port,
        };
        tokio::select! {
            status = session.wait() => bail!("SSH 端口转发在就绪前退出: {}", status?),
            ready = ready_rx => ready.context("SSH 输出已关闭，未收到转发就绪确认")?,
        }
        if let Some(status) = session.child.try_wait()? {
            let _ = session.wait().await;
            bail!("SSH 端口转发在就绪时退出: {status}");
        }
        Ok(session)
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
        let status = self.child
            .wait()
            .await
            .context("等待 SSH 端口转发进程退出失败")?;
        // 进程退出后先读完最后的诊断信息，避免销毁会话时丢失真正的失败原因。
        if let Some(reader) = self.stderr_reader.as_mut() {
            if timeout(Duration::from_secs(1), reader).await.is_ok() {
                self.stderr_reader = None;
            }
        }
        Ok(status)
    }
}

fn forwarding_ready(line: &str, forward_type: ForwardType, expected_remote_reply: &str) -> bool {
    match forward_type {
        ForwardType::Local => line == "debug1: Entering interactive session.",
        ForwardType::Remote => line == expected_remote_reply,
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardState {
    Stopped,
    Connecting,
    Connected,
}

#[derive(Debug)]
pub enum ForwardEvent {
    Log(String),
    State {
        rule_id: Uuid,
        state: ForwardState,
    },
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

fn send_log(events: &UnboundedSender<ForwardEvent>, message: String) {
    let _ = events.send(ForwardEvent::Log(message));
}

fn send_state(
    events: &UnboundedSender<ForwardEvent>,
    rule_id: Uuid,
    state: ForwardState,
) {
    let _ = events.send(ForwardEvent::State { rule_id, state });
}

/// 每条规则独立准备路由、连接和重试；取消操作覆盖所有等待阶段。
async fn supervise_forward(
    rule: PortForwardRule,
    profile: Profile,
    cancel: CancellationToken,
    events: UnboundedSender<ForwardEvent>,
    prepare: PrepareForward,
    options: ForwardOptions,
) {
    let mut retry_delay_secs = INITIAL_RETRY_DELAY_SECS;
    loop {
        send_state(&events, rule.id, ForwardState::Connecting);
        let attempt = async {
            timeout(options.start_timeout, prepare()).await.context("同步转发路由超时")??;
            ForwardSession::start_with_options(&rule, &profile, events.clone(), &options).await
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            result = attempt => result,
        };
        match result {
            Ok(mut session) => {
                retry_delay_secs = INITIAL_RETRY_DELAY_SECS;
                send_state(&events, rule.id, ForwardState::Connected);
                send_log(&events, format!("[forward] 已建立端口转发：{} ({})", rule.name, rule.id));
                let exit = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        let _ = session.stop().await;
                        return;
                    }
                    result = session.wait() => result,
                };
                send_state(&events, rule.id, ForwardState::Connecting);
                send_log(&events, format!("[forward] {} ({}) 已断开：{exit:?}；1 秒后重试", rule.name, rule.id));
            }
            Err(error) => {
                send_log(&events, format!(
                    "[forward] 启动 {} ({}) 失败：{error:#}；{retry_delay_secs} 秒后重试",
                    rule.name, rule.id
                ));
            }
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = sleep(options.retry_interval * retry_delay_secs as u32) => {}
        }
        retry_delay_secs = next_retry_delay(retry_delay_secs);
    }
}

/// 集中管理所有正在运行的端口转发会话。
#[derive(Default)]
pub struct ForwardManager {
    tasks: HashMap<Uuid, ForwardTask>,
    options: ForwardOptions,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(
        &mut self,
        rule: &PortForwardRule,
        profile: &Profile,
        events: UnboundedSender<ForwardEvent>,
    ) {
        self.start_with_prepare(rule, profile, events, || async { Ok(()) }).await;
    }

    /// 注册监督任务后立即返回，网络等待不占用管理锁。
    pub async fn start_with_prepare<F, Fut>(
        &mut self,
        rule: &PortForwardRule,
        profile: &Profile,
        events: UnboundedSender<ForwardEvent>,
        prepare: F,
    ) where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.stop(&rule.id).await;
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(supervise_forward(
            rule.clone(), profile.clone(), cancel.clone(), events,
            Arc::new(move || Box::pin(prepare())), self.options.clone(),
        ));
        self.tasks.insert(rule.id, ForwardTask { cancel, handle });
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
mod tests;

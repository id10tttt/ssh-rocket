use crate::ipc::{HelperCommand, HelperEvent};
use anyhow::{Context, Result, bail};
use std::{net::IpAddr, path::{Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub struct PrivilegedHelperSession {
    child: Child,
    stdin: ChildStdin,
    stdout_lines: Lines<BufReader<ChildStdout>>,
    is_active: bool,
}

impl PrivilegedHelperSession {
    pub async fn ensure_started(helper_path: &Path) -> Result<(Self, Option<ChildStderr>)> {
        let mut command = Command::new("pkexec");
        command
            .arg(helper_path)
            .arg("daemon")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().context("failed to spawn privileged helper via pkexec")?;
        let stdin = child.stdin.take().context("failed to capture helper stdin")?;
        let stdout = child.stdout.take().context("failed to capture helper stdout")?;
        let stderr = child.stderr.take();

        let mut stdout_lines = BufReader::new(stdout).lines();
        let ready_result = timeout(Duration::from_secs(60), async {
            while let Some(line) = stdout_lines.next_line().await? {
                if let Ok(event) = serde_json::from_str::<HelperEvent>(&line) {
                    if matches!(event, HelperEvent::Ready) {
                        return Ok(());
                    }
                }
            }
            bail!("helper stdout closed before ready event")
        })
        .await;

        match ready_result {
            Ok(Ok(())) => {
                let session = Self {
                    child,
                    stdin,
                    stdout_lines,
                    is_active: false,
                };
                Ok((session, stderr))
            }
            Ok(Err(err)) => bail!("privileged helper startup failed: {err}"),
            Err(_) => bail!("timed out waiting for privileged helper authentication"),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// 查询 helper 中透明代理会话的实际运行状态。
    pub async fn check_active(&mut self) -> Result<()> {
        if !self.is_alive() {
            self.is_active = false;
            bail!("privileged helper process exited");
        }
        match self.send_command(&HelperCommand::Status).await? {
            HelperEvent::Status { active: true } => {
                self.is_active = true;
                Ok(())
            }
            HelperEvent::Status { active: false } => {
                self.is_active = false;
                bail!("transparent proxy session is inactive")
            }
            HelperEvent::Error { message } => {
                self.is_active = false;
                bail!("{message}")
            }
            other => bail!("unexpected response from helper: {other:?}"),
        }
    }

    async fn send_command(&mut self, cmd: &HelperCommand) -> Result<HelperEvent> {
        let mut json = serde_json::to_string(cmd)?;
        json.push('\n');
        self.stdin.write_all(json.as_bytes()).await.context("failed to write command to helper")?;
        self.stdin.flush().await.context("failed to flush helper stdin")?;

        let line = timeout(Duration::from_secs(20), async {
            match self.stdout_lines.next_line().await? {
                Some(line) => Ok(line),
                None => bail!("helper stdout closed"),
            }
        })
        .await
        .context("timed out waiting for helper response")??;

        let event = serde_json::from_str::<HelperEvent>(&line)
            .with_context(|| format!("invalid response from helper: {line}"))?;
        Ok(event)
    }

    pub async fn start(
        &mut self,
        config_path: PathBuf,
        uid: u32,
        socks_port: u16,
        dns_port: u16,
        ssh_port: u16,
        ssh_addresses: Vec<IpAddr>,
    ) -> Result<()> {
        let cmd = HelperCommand::Start {
            config_path,
            uid,
            socks_port,
            dns_port,
            ssh_port,
            ssh_addresses,
        };
        match self.send_command(&cmd).await? {
            HelperEvent::Active => {
                self.is_active = true;
                Ok(())
            }
            HelperEvent::Error { message } => bail!("{message}"),
            other => bail!("unexpected response from helper: {other:?}"),
        }
    }

    pub async fn stop(&mut self) -> Result<()> {
        if !self.is_active || !self.is_alive() {
            self.is_active = false;
            return Ok(());
        }
        match self.send_command(&HelperCommand::Stop).await? {
            HelperEvent::Stopped => {
                self.is_active = false;
                Ok(())
            }
            HelperEvent::Error { message } => bail!("{message}"),
            other => bail!("unexpected response from helper: {other:?}"),
        }
    }

    pub async fn sync_rules(&mut self) -> Result<()> {
        if !self.is_active || !self.is_alive() {
            return Ok(());
        }
        let _ = self.send_command(&HelperCommand::SyncRules).await;
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let _ = self.send_command(&HelperCommand::Quit).await;
        if self.is_alive() {
            let _ = self.child.kill().await;
        }
    }
}

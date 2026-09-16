use anyhow::{Context, Result, bail};
use ssh_rocket_core::Profile;
use std::{net::IpAddr, process::Stdio, time::Duration};
use tokio::{net::TcpStream, process::{Child, Command}, time::{sleep, timeout}};

pub struct SshSession {
    child: Child,
    pub socks_port: u16,
    pub dns_port: u16,
}

impl SshSession {
    pub async fn start(profile: &Profile, socks_port: u16, dns_server: IpAddr) -> Result<Self> {
        if profile.host.trim().is_empty() {
            bail!("SSH host is empty");
        }

        let dns_port = socks_port + 1;
        let mut command = Command::new("ssh");
        command
            .arg("-N")
            .arg("-T")
            .arg("-o").arg("ExitOnForwardFailure=yes")
            .arg("-o").arg("ServerAliveInterval=15")
            .arg("-o").arg("ServerAliveCountMax=3")
            .arg("-o").arg("StrictHostKeyChecking=accept-new")
            .arg("-D").arg(format!("127.0.0.1:{socks_port}"))
            .arg("-L").arg(format!("127.0.0.1:{dns_port}:{dns_server}:53"))
            .arg("-p").arg(profile.port.to_string());

        if !profile.username.trim().is_empty() {
            command.arg("-l").arg(profile.username.trim());
        }
        if let Some(identity_file) = &profile.identity_file {
            command.arg("-i").arg(identity_file);
        }
        command
            .arg(profile.host.trim())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().context("failed to start OpenSSH")?;
        tokio::select! {
            status = child.wait() => {
                bail!("SSH exited before SOCKS became ready: {}", status?);
            }
            result = wait_for_tcp(socks_port, Duration::from_secs(15)) => result?,
        }

        Ok(Self { child, socks_port, dns_port })
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.child.id().is_some() {
            self.child.kill().await.context("failed to stop SSH")?;
        }
        Ok(())
    }
}

pub async fn wait_for_tcp(port: u16, deadline: Duration) -> Result<()> {
    timeout(deadline, async move {
        loop {
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("timed out waiting for local SSH tunnel")?;
    Ok(())
}

use anyhow::{Context, Result, bail};
use ssh_rocket_core::Profile;
use std::{collections::HashSet, net::IpAddr, process::Stdio, time::Duration};
use tokio::{
    net::{lookup_host, TcpStream},
    process::{Child, ChildStderr, Command},
    time::{sleep, timeout},
};

pub struct SshSession {
    child: Child,
    pub socks_port: u16,
    pub dns_port: u16,
    pub server_port: u16,
    pub server_addresses: Vec<IpAddr>,
}

impl SshSession {
    pub async fn start(profile: &Profile, socks_port: u16, dns_server: IpAddr) -> Result<Self> {
        if profile.host.trim().is_empty() {
            bail!("SSH host is empty");
        }

        let dns_port = socks_port + 1;
        clean_stale_ssh(socks_port, dns_port).await;

        let (server_host, server_port) = effective_server(profile).await;
        let server_addresses = lookup_host((server_host.as_str(), server_port))
            .await
            .map(|addresses| {
                addresses
                    .map(|address| address.ip())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .context("failed to resolve the SSH server address")?;
        if server_addresses.is_empty() {
            bail!("the SSH server resolved to no addresses");
        }

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

        let mut child = command.spawn().context("failed to start OpenSSH")?;
        tokio::select! {
            status = child.wait() => {
                bail!("SSH exited before SOCKS became ready: {}", status?);
            }
            result = wait_for_tcp(socks_port, Duration::from_secs(15)) => result?,
        }

        Ok(Self {
            child,
            socks_port,
            dns_port,
            server_port,
            server_addresses,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.child.id().is_some() {
            self.child.kill().await.context("failed to stop SSH")?;
        }
        Ok(())
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.child.wait().await.context("failed to wait for SSH")
    }
}

/// 解析 OpenSSH 配置后的真实服务器地址，确保运行时连接不会被重新送入代理。
async fn effective_server(profile: &Profile) -> (String, u16) {
    let mut command = Command::new("ssh");
    command.arg("-G").arg("-p").arg(profile.port.to_string());
    if !profile.username.trim().is_empty() {
        command.arg("-l").arg(profile.username.trim());
    }
    if let Some(identity_file) = &profile.identity_file {
        command.arg("-i").arg(identity_file);
    }
    let output = command.arg(profile.host.trim()).output().await;
    let Ok(output) = output else {
        return (profile.host.trim().to_string(), profile.port);
    };
    if !output.status.success() {
        return (profile.host.trim().to_string(), profile.port);
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let host = text
        .lines()
        .find_map(|line| line.strip_prefix("hostname "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(profile.host.trim())
        .to_string();
    let port = text
        .lines()
        .find_map(|line| line.strip_prefix("port "))
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(profile.port);
    (host, port)
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

async fn clean_stale_ssh(socks_port: u16, dns_port: u16) {
    let my_uid = unsafe { libc::getuid() };
    let my_pid = std::process::id() as i32;
    if let Ok(mut entries) = tokio::fs::read_dir("/proc").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else { continue; };
            if pid == my_pid { continue; }
            let status = tokio::fs::read_to_string(format!("/proc/{pid}/status")).await.unwrap_or_default();
            if !status.lines().any(|line| line == format!("Uid:\t{}\t{}\t{}\t{}", my_uid, my_uid, my_uid, my_uid))
                && !status.lines().any(|line| line.starts_with(&format!("Uid:\t{}\t", my_uid)))
            {
                continue;
            }
            if let Ok(cmdline) = tokio::fs::read_to_string(format!("/proc/{pid}/cmdline")).await {
                if cmdline.contains("ssh") && (cmdline.contains(&format!(":{socks_port}")) || cmdline.contains(&format!(":{dns_port}"))) {
                    unsafe { libc::kill(pid, libc::SIGKILL); }
                }
            }
        }
    }
    sleep(Duration::from_millis(50)).await;
}

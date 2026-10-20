use super::*;
use std::{fs, net::TcpListener, os::unix::fs::PermissionsExt, sync::atomic::{AtomicBool, Ordering}};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

struct Fixture {
    dir: PathBuf,
    rule: PortForwardRule,
    profile: Profile,
    manager: ForwardManager,
    events: UnboundedSender<ForwardEvent>,
    receiver: UnboundedReceiver<ForwardEvent>,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ssh-rocket-forward-test-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let program = dir.join("ssh");
        fs::write(&program, include_str!("mock_ssh.sh")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.join("mode"), mode).unwrap();
        let rule = PortForwardRule {
            forward_type: ForwardType::Remote,
            local_port: 18080,
            remote_port: 28080,
            ..PortForwardRule::default()
        };
        let profile = Profile { host: dir.to_string_lossy().into(), ..Profile::default() };
        let manager = ForwardManager {
            options: ForwardOptions {
                ssh_program: program,
                start_timeout: Duration::from_millis(350),
                retry_interval: Duration::from_millis(15),
            },
            ..ForwardManager::default()
        };
        let (events, receiver) = unbounded_channel();
        Self { dir, rule, profile, manager, events, receiver }
    }

    fn mode(&self, mode: &str) {
        fs::write(self.dir.join("mode"), mode).unwrap();
    }

    fn attempts(&self) -> usize {
        fs::read_to_string(self.dir.join("attempts")).unwrap_or_default().lines().count()
    }

    async fn start(&mut self) {
        self.manager.start(&self.rule, &self.profile, self.events.clone()).await;
    }

    async fn state(&mut self, expected: ForwardState) {
        timeout(Duration::from_secs(5), async {
            loop {
                match self.receiver.recv().await {
                    Some(ForwardEvent::State { rule_id, state }) if rule_id == self.rule.id && state == expected => break,
                    Some(ForwardEvent::Log(line)) => eprintln!("{line}"),
                    None => panic!("转发事件通道意外关闭"),
                    _ => {}
                }
            }
        }).await.expect("未收到预期的转发状态");
    }

    async fn no_connected(&mut self, duration: Duration) {
        let result = timeout(duration, async {
            loop {
                if let Some(ForwardEvent::State { state: ForwardState::Connected, .. }) = self.receiver.recv().await {
                    break;
                }
            }
        }).await;
        assert!(result.is_err(), "不应提前报告已连接");
    }

    async fn stop(&mut self) {
        self.manager.stop_all().await;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 仅清理本测试创建的 UUID 临时目录。
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn retry_delay_uses_exponential_backoff_with_cap() {
    assert_eq!(next_retry_delay(1), 2);
    assert_eq!(next_retry_delay(16), 30);
    assert_eq!(next_retry_delay(MAX_RETRY_DELAY_SECS), MAX_RETRY_DELAY_SECS);
}

#[test]
fn shell_quote_escapes_single_quotes() {
    assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
}

#[test]
fn readiness_requires_the_requested_forward_confirmation() {
    assert!(!forwarding_ready("debug1: Local forwarding listening on 127.0.0.1 port 80.", ForwardType::Local, ""));
    assert!(forwarding_ready("debug1: Entering interactive session.", ForwardType::Local, ""));
    let expected = "debug1: remote forward success for: listen 127.0.0.1:28080, connect 127.0.0.1:18080";
    assert!(forwarding_ready(expected, ForwardType::Remote, expected));
    assert!(!forwarding_ready("debug1: Entering interactive session.", ForwardType::Remote, expected));
    assert!(!forwarding_ready("debug1: remote forward success for: listen 127.0.0.1:1, connect 127.0.0.1:2", ForwardType::Remote, expected));
}

#[tokio::test]
async fn startup_without_network_retries_until_network_returns() {
    let mut fixture = Fixture::new("offline");
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(150)).await;
    assert!(fixture.attempts() >= 2);
    fixture.mode("ready");
    fixture.state(ForwardState::Connected).await;
    fixture.stop().await;
}

#[tokio::test]
async fn startup_errors_are_reported_before_retry() {
    let mut fixture = Fixture::new("offline");
    fixture.start().await;
    timeout(Duration::from_secs(2), async {
        loop {
            match fixture.receiver.recv().await {
                Some(ForwardEvent::Log(line)) if line.contains("connect failed: offline") => break,
                Some(ForwardEvent::State { state: ForwardState::Connected, .. }) => panic!("错误连接状态"),
                _ => {}
            }
        }
    }).await.expect("应保留 SSH 退出前的错误输出");
    fixture.stop().await;
}

#[tokio::test]
async fn network_interruption_recovers_without_user_action() {
    let mut fixture = Fixture::new("ready");
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    fixture.mode("offline");
    fixture.state(ForwardState::Connecting).await;
    fixture.no_connected(Duration::from_millis(150)).await;
    fixture.mode("ready");
    fixture.state(ForwardState::Connected).await;
    assert!(fixture.attempts() >= 2);
    fixture.stop().await;
}

#[tokio::test]
async fn server_session_exit_is_restarted() {
    let mut fixture = Fixture::new("ready");
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    let pid = fs::read_to_string(fixture.dir.join("attempts")).unwrap().trim().parse::<i32>().unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    fixture.state(ForwardState::Connecting).await;
    fixture.state(ForwardState::Connected).await;
    assert!(fixture.attempts() >= 2);
    fixture.stop().await;
}

#[tokio::test]
async fn remote_port_conflict_retries_without_cleanup_commands() {
    let mut fixture = Fixture::new("conflict");
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(150)).await;
    assert!(fixture.attempts() >= 2);
    assert!(!fixture.dir.join("unexpected_command").exists());
    fixture.mode("ready");
    fixture.state(ForwardState::Connected).await;
    fixture.stop().await;
}

#[tokio::test]
async fn local_port_conflict_preserves_listener_and_recovers_after_release() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut fixture = Fixture::new("ready");
    fixture.rule.forward_type = ForwardType::Local;
    fixture.rule.local_port = listener.local_addr().unwrap().port();
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(100)).await;
    assert_eq!(fixture.attempts(), 0);
    assert!(std::net::TcpStream::connect(listener.local_addr().unwrap()).is_ok());
    drop(listener);
    fixture.state(ForwardState::Connected).await;
    fixture.stop().await;
}

#[tokio::test]
async fn saving_updated_credentials_replaces_the_old_retry_task() {
    let mut fixture = Fixture::new("ready");
    fs::write(fixture.dir.join("allowed_user"), "new_user").unwrap();
    fixture.profile.username = "old_user".into();
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(100)).await;
    fixture.profile.username = "new_user".into();
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_eq!(fixture.manager.active_rule_ids(), vec![fixture.rule.id]);
    assert_eq!(fs::read_to_string(fixture.dir.join("connected_user")).unwrap().trim(), "new_user");
    fixture.stop().await;
}

#[tokio::test]
async fn saving_updated_profile_replaces_an_established_session() {
    let mut fixture = Fixture::new("ready");
    fixture.profile.username = "old_user".into();
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    let old_pid = fs::read_to_string(fixture.dir.join("attempts")).unwrap().trim().parse::<i32>().unwrap();
    fixture.profile.username = "new_user".into();
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_eq!(fixture.manager.active_rule_ids(), vec![fixture.rule.id]);
    assert_eq!(fs::read_to_string(fixture.dir.join("connected_user")).unwrap().trim(), "new_user");
    assert_eq!(unsafe { libc::kill(old_pid, 0) }, -1, "旧会话必须退出");
    fixture.stop().await;
}

#[tokio::test]
async fn slow_forward_does_not_block_other_forwards_or_stopping() {
    let mut slow = Fixture::new("hang");
    let mut healthy = Fixture::new("ready");
    // 同一个管理器中并存的任务使用同一测试 SSH 程序，但各自读取独立故障状态。
    slow.start().await;
    timeout(Duration::from_millis(100), slow.manager.start(
        &healthy.rule, &healthy.profile, healthy.events.clone()
    )).await.expect("慢连接不应占用管理器");
    healthy.state(ForwardState::Connected).await;
    timeout(Duration::from_millis(100), slow.manager.stop_all()).await.expect("连接阶段应可取消");
}

#[tokio::test]
async fn authentication_hang_times_out_and_retries_without_false_connected() {
    let mut fixture = Fixture::new("hang");
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(850)).await;
    assert!(fixture.attempts() >= 2);
    fixture.mode("ready");
    fixture.state(ForwardState::Connected).await;
    fixture.stop().await;
}

#[tokio::test]
async fn failed_route_sync_remains_supervised_and_recovers() {
    let mut fixture = Fixture::new("ready");
    let available = Arc::new(AtomicBool::new(false));
    let gate = available.clone();
    fixture.manager.start_with_prepare(&fixture.rule, &fixture.profile, fixture.events.clone(), move || {
        let gate = gate.clone();
        async move {
            if !gate.load(Ordering::SeqCst) { bail!("模拟路由同步失败"); }
            Ok(())
        }
    }).await;
    fixture.no_connected(Duration::from_millis(100)).await;
    assert!(fixture.manager.is_running(&fixture.rule.id));
    assert_eq!(fixture.attempts(), 0);
    available.store(true, Ordering::SeqCst);
    fixture.state(ForwardState::Connected).await;
    fixture.stop().await;
}

#[tokio::test]
async fn pending_route_sync_can_be_cancelled_without_starting_ssh() {
    let mut fixture = Fixture::new("ready");
    fixture.manager.start_with_prepare(&fixture.rule, &fixture.profile, fixture.events.clone(), || async {
        std::future::pending::<Result<()>>().await
    }).await;
    fixture.state(ForwardState::Connecting).await;
    timeout(Duration::from_millis(100), fixture.manager.stop_all()).await.expect("路由准备阶段应可取消");
    assert_eq!(fixture.attempts(), 0);
    assert!(!fixture.manager.is_running(&fixture.rule.id));
}

#[tokio::test]
async fn stderr_is_drained_beyond_pipe_capacity_and_errors_are_reported() {
    let mut fixture = Fixture::new("flood");
    fixture.start().await;
    let mut diagnostic_received = false;
    timeout(Duration::from_secs(5), async {
        while let Some(event) = fixture.receiver.recv().await {
            match event {
                ForwardEvent::Log(line) if line.contains("Connection refused") => diagnostic_received = true,
                ForwardEvent::State { state: ForwardState::Connected, .. } => break,
                _ => {}
            }
        }
    }).await.expect("大量 stderr 不应阻塞连接");
    assert!(diagnostic_received);
    fixture.stop().await;
}

struct LocalSshd {
    child: Option<std::process::Child>,
    config_path: PathBuf,
    port: u16,
}

impl LocalSshd {
    async fn start(&mut self) {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/usr/sbin/sshd");
        command.args(["-D", "-e", "-f"]).arg(&self.config_path)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit());
        unsafe { command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 { return Err(std::io::Error::last_os_error()); }
            Ok(())
        }); }
        self.child = Some(command.spawn().expect("启动隔离 sshd"));
        timeout(Duration::from_secs(3), async {
            loop {
                assert!(self.child.as_mut().unwrap().try_wait().unwrap().is_none(), "测试 sshd 提前退出");
                if tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await.is_ok() { break; }
                sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("测试 sshd 未开始监听");
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // sshd 会为已认证会话创建独立进程组，按本测试子进程树逐一终止。
            fn descendants(pid: u32, result: &mut Vec<i32>) {
                let children = fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
                for child in children.split_whitespace().filter_map(|pid| pid.parse::<u32>().ok()) {
                    descendants(child, result);
                    result.push(child as i32);
                }
            }
            let mut children = Vec::new();
            descendants(child.id(), &mut children);
            for pid in children { unsafe { libc::kill(pid, libc::SIGTERM); } }
            unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM); }
            let _ = child.wait();
        }
    }
}

impl Drop for LocalSshd {
    fn drop(&mut self) { self.stop(); }
}

fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn assert_forwarded_echo(port: u16) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    timeout(Duration::from_secs(3), async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(b"forward-ok").await.unwrap();
        let mut reply = [0; 10];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"forward-ok");
    }).await.expect("真实隧道无法传输数据");
}

/// 使用临时密钥和回环地址验证真实 OpenSSH；不访问用户 SSH 配置或远程服务器。
#[tokio::test]
#[ignore = "需要本机 /usr/sbin/sshd、ssh-keygen 和 ssh"]
async fn real_openssh_forwards_recovers_from_restart_and_preserves_conflicting_listener() {
    let mut fixture = Fixture::new("ready");
    let host_key = fixture.dir.join("host_key");
    let client_key = fixture.dir.join("client_key");
    for key in [&host_key, &client_key] {
        assert!(std::process::Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(key).status().unwrap().success());
    }
    let username = String::from_utf8(std::process::Command::new("id").arg("-un").output().unwrap().stdout).unwrap();
    let port = unused_port();
    let config_path = fixture.dir.join("sshd_config");
    fs::write(&config_path, format!(
        "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nStrictModes no\nPidFile {}\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nAllowTcpForwarding yes\nPermitRootLogin yes\nUseDNS no\nLogLevel ERROR\n",
        host_key.display(), client_key.with_extension("pub").display(), fixture.dir.join("sshd.pid").display()
    )).unwrap();
    let wrapper = fixture.dir.join("real_ssh");
    fs::write(&wrapper, format!(
        "#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile={} -o IdentitiesOnly=yes -o ServerAliveInterval=1 -o ServerAliveCountMax=1 \"$@\"\n",
        shell_quote_path(&fixture.dir.join("known_hosts"))
    )).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    fixture.manager.options.ssh_program = wrapper;
    fixture.manager.options.start_timeout = Duration::from_secs(3);
    fixture.manager.options.retry_interval = Duration::from_millis(30);
    fixture.profile.host = "127.0.0.1".into();
    fixture.profile.port = port;
    fixture.profile.username = username.trim().into();
    fixture.profile.identity_file = Some(client_key.clone());
    // 只断开测试 TCP 中继即可模拟断网，无需修改主机网络或防火墙。
    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.profile.port = relay_listener.local_addr().unwrap().port();
    let network_available = Arc::new(AtomicBool::new(true));
    let network_sessions = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
    let relay_available = network_available.clone();
    let relay_sessions = network_sessions.clone();
    let relay_task = tokio::spawn(async move {
        loop {
            let (mut incoming, _) = relay_listener.accept().await.unwrap();
            if !relay_available.load(Ordering::SeqCst) { continue; }
            let cancel = relay_sessions.lock().unwrap().clone();
            tokio::spawn(async move {
                if let Ok(mut outgoing) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                    tokio::select! {
                        _ = cancel.cancelled() => {}
                        _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing) => {}
                    }
                }
            });
        }
    });
    let echo_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo_listener.local_addr().unwrap().port();
    let echo_task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = echo_listener.accept().await.unwrap();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0; 128];
                while let Ok(length) = stream.read(&mut buffer).await {
                    if length == 0 { break; }
                    if stream.write_all(&buffer[..length]).await.is_err() { break; }
                }
            });
        }
    });
    let mut server = LocalSshd { child: None, config_path, port };
    // 服务尚未启动：不能假报已连接，启动后无需用户操作即可恢复。
    fixture.rule.local_port = echo_port;
    fixture.rule.remote_port = unused_port();
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(150)).await;
    server.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.remote_port).await;
    network_available.store(false, Ordering::SeqCst);
    network_sessions.lock().unwrap().cancel();
    fixture.state(ForwardState::Connecting).await;
    fixture.no_connected(Duration::from_millis(150)).await;
    *network_sessions.lock().unwrap() = CancellationToken::new();
    network_available.store(true, Ordering::SeqCst);
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.remote_port).await;
    // 重启服务器，同时断开已接受的会话，验证原规则自动重连。
    server.stop();
    fixture.state(ForwardState::Connecting).await;
    server.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.remote_port).await;
    fixture.stop().await;
    // 保持另一个真实监听者存活，冲突期间只重试，释放后建立转发。
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    fixture.rule.remote_port = occupied.local_addr().unwrap().port();
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(200)).await;
    assert!(std::net::TcpStream::connect(occupied.local_addr().unwrap()).is_ok());
    drop(occupied);
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.remote_port).await;
    fixture.stop().await;
    // 先使用不被服务器接受的密钥，再用最新凭据替换监督任务。
    fixture.profile.identity_file = Some(host_key);
    fixture.start().await;
    fixture.no_connected(Duration::from_millis(200)).await;
    fixture.profile.identity_file = Some(client_key);
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.remote_port).await;
    fixture.stop().await;
    // 本地转发也必须在报告就绪后能传输数据。
    fixture.rule.forward_type = ForwardType::Local;
    fixture.rule.local_port = unused_port();
    fixture.rule.remote_port = echo_port;
    // 另一条规则等待 SSH 握手时，正常规则必须仍能启动。
    let blackhole = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut slow_profile = fixture.profile.clone();
    slow_profile.port = blackhole.local_addr().unwrap().port();
    let slow_rule = PortForwardRule {
        forward_type: ForwardType::Remote,
        local_port: echo_port,
        remote_port: unused_port(),
        ..PortForwardRule::default()
    };
    fixture.manager.start(&slow_rule, &slow_profile, fixture.events.clone()).await;
    fixture.start().await;
    fixture.state(ForwardState::Connected).await;
    assert_forwarded_echo(fixture.rule.local_port).await;
    fixture.stop().await;
    echo_task.abort();
    network_sessions.lock().unwrap().cancel();
    relay_task.abort();
}

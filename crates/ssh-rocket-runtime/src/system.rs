use anyhow::{Context, Result, bail};
use ipnet::IpNet;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    time::{sleep, timeout},
};

pub const TUN_NAME: &str = "sshrocket0";
pub const MARK: &str = "0x5352";
pub const TABLE: &str = "21330";
pub const DNS_RULE_PRIORITY: &str = "21328";
pub const NFT_TABLE: &str = "ssh_rocket";
pub const DOMAIN_RULE_TIMEOUT_SECS: u32 = 1800;
pub const TUN_DNS_ADDRESS: &str = "10.0.0.33";
pub const MAX_TUN_RETRIES: usize = 3;
pub const SYSTEM_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
pub const CGROUPS: [&str; 3] = ["sshrocket-proxy", "sshrocket-direct", "sshrocket-block"];

pub struct SystemState {
    pub uid: u32,
    pub socks_port: u16,
    pub dns_port: u16,
    pub ssh_port: u16,
    pub ssh_addresses: Vec<IpAddr>,
    pub config_path: PathBuf,
    pub config_mtime: Option<std::time::SystemTime>,
    pub config: AppConfig,
    current_pids: HashMap<u32, &'static str>,
}

impl SystemState {
    pub fn new(
        uid: u32,
        socks_port: u16,
        dns_port: u16,
        ssh_port: u16,
        ssh_addresses: Vec<IpAddr>,
        config_path: PathBuf,
        config: AppConfig,
    ) -> Self {
        let config_mtime = std::fs::metadata(&config_path).and_then(|m| m.modified()).ok();
        Self {
            uid,
            socks_port,
            dns_port,
            ssh_port,
            ssh_addresses,
            config_path,
            config_mtime,
            config,
            current_pids: HashMap::new(),
        }
    }

    pub async fn setup(&mut self) -> Result<()> {
        let _ = self.run_ip(&["-4", "rule", "del", "priority", DNS_RULE_PRIORITY]).await;
        let _ = self.run_ip(&["-4", "rule", "del", "priority", "21329"]).await;
        let _ = self.run_ip(&["-4", "rule", "del", "priority", TABLE]).await;
        let _ = self.run_ip(&["-6", "rule", "del", "priority", TABLE]).await;
        self.configure_tun_interface().await?;
        let _ = self.run_ip(&["-4", "route", "replace", "198.18.0.0/15", "dev", TUN_NAME, "table", TABLE]).await;
        self.run_ip(&["-4", "route", "replace", "default", "dev", TUN_NAME, "table", TABLE]).await?;
        self.run_ip(&[
            "-4",
            "rule",
            "add",
            "priority",
            DNS_RULE_PRIORITY,
            "uidrange",
            "0-0",
            "to",
            "10.0.0.1/32",
            "lookup",
            TABLE,
        ])
        .await?;
        self.run_ip(&["-4", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE]).await?;
        let _ = self.run_ip(&["-4", "rule", "add", "priority", "21329", "to", "198.18.0.0/15", "lookup", TABLE]).await;
        if self.config.settings.ipv6 {
            self.run_ip(&["-6", "route", "replace", "default", "dev", TUN_NAME, "table", TABLE]).await?;
            self.run_ip(&["-6", "rule", "add", "priority", TABLE, "fwmark", MARK, "lookup", TABLE]).await?;
        }
        self.setup_cgroups().await?;
        self.install_nftables().await?;
        self.assign_apps().await?;
        Ok(())
    }

    pub async fn cleanup(&self) {
        let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
        let _ = command("resolvectl", &["revert", TUN_NAME]).await;
        let _ = command("ip", &["-4", "rule", "del", "priority", DNS_RULE_PRIORITY]).await;
        let _ = command("ip", &["-4", "rule", "del", "priority", "21329"]).await;
        let _ = command("ip", &["-4", "rule", "del", "priority", TABLE]).await;
        let _ = command("ip", &["-6", "rule", "del", "priority", TABLE]).await;
        let _ = command("ip", &["-4", "route", "flush", "table", TABLE]).await;
        let _ = command("ip", &["-6", "route", "flush", "table", TABLE]).await;
        let _ = command("ip", &["-4", "route", "del", "10.0.0.1/32", "dev", TUN_NAME]).await;
        self.restore_app_processes().await;
        for name in CGROUPS {
            let path = format!("/sys/fs/cgroup/{name}");
            let _ = tokio::fs::remove_dir(path).await;
        }
    }

    async fn run_ip(&self, args: &[&str]) -> Result<()> {
        command("ip", args).await
    }

    /// 仅在链路或地址状态不符合预期时更新 TUN，避免重复产生 Netlink 通知。
    async fn configure_tun_interface(&self) -> Result<()> {
        let flags = tokio::fs::read_to_string(format!("/sys/class/net/{TUN_NAME}/flags"))
            .await
            .with_context(|| format!("failed to read flags for {TUN_NAME}"))?;
        let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16)
            .with_context(|| format!("invalid flags for {TUN_NAME}"))?;
        if flags & libc::IFF_UP as u32 == 0 {
            self.run_ip(&["link", "set", "dev", TUN_NAME, "up"]).await?;
        }

        let addresses = command_output("ip", &["-4", "-o", "addr", "show", "dev", TUN_NAME]).await?;
        let stale_addresses = addresses
            .split_whitespace()
            .filter(|value| value.starts_with("10.0.0.33/") && *value != "10.0.0.33/32")
            .map(str::to_string)
            .collect::<Vec<_>>();
        for address in stale_addresses {
            let _ = self.run_ip(&["-4", "addr", "del", &address, "dev", TUN_NAME]).await;
        }
        if !addresses.split_whitespace().any(|value| value == "10.0.0.33/32") {
            self.run_ip(&["-4", "addr", "replace", "10.0.0.33/32", "dev", TUN_NAME])
                .await?;
        }
        Ok(())
    }

    async fn setup_cgroups(&self) -> Result<()> {
        for name in CGROUPS {
            tokio::fs::create_dir_all(format!("/sys/fs/cgroup/{name}"))
                .await
                .with_context(|| format!("failed to create cgroup {name}"))?;
        }
        Ok(())
    }

    pub async fn configure_system_resolver(&self) -> Result<()> {
        command("resolvectl", &["dns", TUN_NAME, TUN_DNS_ADDRESS]).await?;
        command("resolvectl", &["domain", TUN_NAME, "~."]).await?;
        command("resolvectl", &["default-route", TUN_NAME, "yes"]).await
    }

    pub async fn assign_apps(&mut self) -> Result<()> {
        let rules: HashMap<_, _> = self
            .config
            .settings
            .app_rules
            .iter()
            .map(|rule| (rule.executable.clone(), rule.action))
            .collect();
        let mut desired_pids = HashMap::new();
        if !rules.is_empty() {
            let mut entries = tokio::fs::read_dir("/proc").await?;
            while let Some(entry) = entries.next_entry().await? {
                let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else { continue; };
                let status = tokio::fs::read_to_string(format!("/proc/{pid}/status")).await.unwrap_or_default();
                if !status.lines().any(|line| line.starts_with(&format!("Uid:\t{}\t", self.uid))) {
                    continue;
                }
                let Ok(executable) = tokio::fs::read_link(format!("/proc/{pid}/exe")).await else { continue; };
                let Some(action) = app_action(&rules, &executable) else { continue; };
                let group = match action {
                    RuleAction::Proxy => "sshrocket-proxy",
                    RuleAction::Direct => "sshrocket-direct",
                    RuleAction::Block => "sshrocket-block",
                };
                desired_pids.insert(pid, group);
            }
        }

        self.current_pids = read_cgroup_pids().await;
        for pid in self.current_pids.keys().copied().collect::<Vec<_>>() {
            if !desired_pids.contains_key(&pid) {
                let _ = tokio::fs::write("/sys/fs/cgroup/cgroup.procs", pid.to_string()).await;
                self.current_pids.remove(&pid);
            }
        }
        for (pid, group) in desired_pids {
            if self.current_pids.get(&pid) == Some(&group) {
                continue;
            }
            if tokio::fs::write(format!("/sys/fs/cgroup/{group}/cgroup.procs"), pid.to_string())
                .await
                .is_ok()
            {
                self.current_pids.insert(pid, group);
            }
        }
        Ok(())
    }

    async fn restore_app_processes(&self) {
        for group in CGROUPS {
            let Ok(pids) = tokio::fs::read_to_string(format!("/sys/fs/cgroup/{group}/cgroup.procs")).await else {
                continue;
            };
            for pid in pids.lines() {
                let _ = tokio::fs::write("/sys/fs/cgroup/cgroup.procs", pid).await;
            }
        }
    }

    async fn install_nftables(&self) -> Result<()> {
        let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
        command("nft", &["add", "table", "inet", NFT_TABLE]).await?;
        command("nft", &["add", "chain", "inet", NFT_TABLE, "output", "{", "type", "route", "hook", "output", "priority", "mangle", ";", "policy", "accept", ";", "}"]).await?;
        command("nft", &["add", "chain", "inet", NFT_TABLE, "dns_output", "{", "type", "nat", "hook", "output", "priority", "dstnat", ";", "policy", "accept", ";", "}"]).await?;

        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "skuid", "!=", &self.uid.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "fib", "daddr", "type", "local", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "{", "127.0.0.0/8", ",", "169.254.0.0/16", ",", "192.168.0.0/16", ",", "172.16.0.0/12", ",", "10.0.0.0/8", ",", "224.0.0.0/4", ",", "255.255.255.255", "}", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip6", "daddr", "{", "::1", ",", "fc00::/7", ",", "fe80::/10", ",", "ff00::/8", "}", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "tcp", "dport", &self.socks_port.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "tcp", "dport", &self.dns_port.to_string(), "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "223.5.5.5", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "114.114.114.114", "return"]).await?;
        for address in &self.ssh_addresses {
            let family = if address.is_ipv4() { "ip" } else { "ip6" };
            let address = address.to_string();
            command(
                "nft",
                &[
                    "add",
                    "rule",
                    "inet",
                    NFT_TABLE,
                    "output",
                    family,
                    "daddr",
                    &address,
                    "tcp",
                    "dport",
                    &self.ssh_port.to_string(),
                    "return",
                ],
            )
            .await?;
        }
        command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "ip", "daddr", "198.18.0.0/15", "meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "dns_output", "meta", "skuid", "0", "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "dns_output", "ip", "daddr", TUN_DNS_ADDRESS, "return"]).await?;
        command("nft", &["add", "rule", "inet", NFT_TABLE, "dns_output", "udp", "dport", "53", "redirect", "to", &format!(":{}", crate::DNS_ROUTER_PORT)]).await?;

        self.install_ip_rules(&self.config.settings.custom_overrides).await?;
        self.install_cgroup_rules().await?;
        self.install_domain_sets().await?;
        self.install_ip_rules(&self.config.settings.ip_rules).await?;
        self.install_ip_rules(&self.config.settings.imported_ip_rules).await?;

        match self.config.settings.default_policy {
            RuleAction::Proxy => {
                command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "udp", "dport", "443", "reject"]).await?;
                command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "l4proto", "tcp", "meta", "mark", "set", MARK]).await?;
            }
            RuleAction::Block => command("nft", &["add", "rule", "inet", NFT_TABLE, "output", "meta", "l4proto", "tcp", "reject"]).await?,
            RuleAction::Direct => {}
        }
        Ok(())
    }

    async fn install_domain_sets(&self) -> Result<()> {
        for (name, address_type) in [
            ("domain_proxy4", "ipv4_addr"),
            ("domain_direct4", "ipv4_addr"),
            ("domain_block4", "ipv4_addr"),
            ("domain_proxy6", "ipv6_addr"),
            ("domain_direct6", "ipv6_addr"),
            ("domain_block6", "ipv6_addr"),
        ] {
            command("nft", &["add", "set", "inet", NFT_TABLE, name, "{", "type", address_type, ";", "flags", "timeout", ";", "}"]).await?;
        }

        for family in ["ip", "ip6"] {
            let suffix = if family == "ip" { "4" } else { "6" };
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_block{suffix}"), "reject"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_direct{suffix}"), "return"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_proxy{suffix}"), "udp", "dport", "443", "reject"]).await?;
            command("nft", &["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", &format!("@domain_proxy{suffix}"), "meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]).await?;
        }
        Ok(())
    }

    async fn install_cgroup_rules(&self) -> Result<()> {
        for (group, action) in [
            ("sshrocket-block", RuleAction::Block),
            ("sshrocket-direct", RuleAction::Direct),
            ("sshrocket-proxy", RuleAction::Proxy),
        ] {
            let mut args = vec!["add", "rule", "inet", NFT_TABLE, "output", "socket", "cgroupv2", "level", "1", group];
            match action {
                RuleAction::Block => {
                    args.push("reject");
                    command("nft", &args).await?;
                }
                RuleAction::Direct => {
                    args.push("return");
                    command("nft", &args).await?;
                }
                RuleAction::Proxy => {
                    let mut reject_quic = args.clone();
                    reject_quic.extend(["udp", "dport", "443", "reject"]);
                    command("nft", &reject_quic).await?;
                    args.extend(["meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]);
                    command("nft", &args).await?;
                }
            }
        }
        Ok(())
    }

    async fn install_ip_rules(&self, rules: &[ssh_rocket_core::IpRule]) -> Result<()> {
        for rule in rules {
            let family = match rule.network {
                IpNet::V4(_) => "ip",
                IpNet::V6(_) => "ip6",
            };
            let network = rule.network.to_string();
            let mut args = vec!["add", "rule", "inet", NFT_TABLE, "output", family, "daddr", network.as_str()];
            match rule.action {
                RuleAction::Block => {
                    args.push("reject");
                    command("nft", &args).await?;
                }
                RuleAction::Direct => {
                    args.push("return");
                    command("nft", &args).await?;
                }
                RuleAction::Proxy => {
                    let mut reject_quic = args.clone();
                    reject_quic.extend(["udp", "dport", "443", "reject"]);
                    command("nft", &reject_quic).await?;
                    args.extend(["meta", "l4proto", "tcp", "meta", "mark", "set", MARK, "return"]);
                    command("nft", &args).await?;
                }
            }
        }
        Ok(())
    }
}

pub fn app_action(rules: &HashMap<PathBuf, RuleAction>, executable: &Path) -> Option<RuleAction> {
    if let Some(action) = rules.get(executable) {
        return Some(*action);
    }
    let executable_name = executable.file_name()?;
    rules
        .iter()
        .find(|(rule, _)| rule.file_name() == Some(executable_name))
        .map(|(_, action)| *action)
}

pub async fn read_cgroup_pids() -> HashMap<u32, &'static str> {
    let mut current_pids = HashMap::new();
    for group in CGROUPS {
        let Ok(pids) = tokio::fs::read_to_string(format!("/sys/fs/cgroup/{group}/cgroup.procs")).await else {
            continue;
        };
        for pid in pids.lines().filter_map(|pid| pid.parse::<u32>().ok()) {
            current_pids.insert(pid, group);
        }
    }
    current_pids
}

pub async fn update_domain_addresses(addresses: &[IpAddr], action: RuleAction) {
    if addresses.is_empty() {
        return;
    }
    let target_cat = match action {
        RuleAction::Proxy => "proxy",
        RuleAction::Direct => "direct",
        RuleAction::Block => "block",
    };
    let mut batch = String::new();
    for address in addresses {
        let suffix = if address.is_ipv4() { "4" } else { "6" };
        let value = address.to_string();
        batch.push_str(&format!("add element inet {NFT_TABLE} domain_{target_cat}{suffix} {{ {value} timeout {DOMAIN_RULE_TIMEOUT_SECS}s }}\n"));
    }
    if let Ok(mut child) = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(batch.as_bytes()).await;
        }
        if let Ok(output) = child.wait_with_output().await {
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.contains("No such file or directory") {
                    eprintln!("[error] [nft] update_domain_addresses failed: {err}");
                }
            }
        }
    }
}

/// 创建与 tun2proxy packet-information 模式兼容的常驻 TUN，后续会话只重新附加。
pub async fn ensure_persistent_tun() -> Result<()> {
    let tun_path = format!("/sys/class/net/{TUN_NAME}/tun_flags");
    if !Path::new(&tun_path).exists() {
        if Path::new(&format!("/sys/class/net/{TUN_NAME}")).exists() {
            bail!("network interface {TUN_NAME} exists but is not a TUN device");
        }
        command("ip", &["tuntap", "add", "dev", TUN_NAME, "mode", "tun", "pi"])
            .await
            .with_context(|| format!("failed to create persistent TUN {TUN_NAME}"))?;
    }

    let flags = tokio::fs::read_to_string(&tun_path)
        .await
        .with_context(|| format!("failed to read TUN flags for {TUN_NAME}"))?;
    let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16)
        .with_context(|| format!("invalid TUN flags for {TUN_NAME}"))?;
    const IFF_TUN: u32 = 0x0001;
    const IFF_NO_PI: u32 = 0x1000;
    if flags & IFF_TUN == 0 || flags & IFF_NO_PI != 0 {
        bail!("existing TUN {TUN_NAME} is incompatible with tun2proxy packet-information mode");
    }
    Ok(())
}

pub async fn wait_for_interface() -> Result<()> {
    for _ in 0..100 {
        if Path::new(&format!("/sys/class/net/{TUN_NAME}")).exists() {
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }
    bail!("TUN interface {TUN_NAME} was not created")
}

pub async fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = timeout(SYSTEM_COMMAND_TIMEOUT, command.output())
        .await
        .with_context(|| format!("timed out executing {program} {}", args.join(" ")))??;
    if !output.status.success() {
        bail!("{} {} failed: {}", program, args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    String::from_utf8(output.stdout).with_context(|| format!("{program} output was not valid UTF-8"))
}

pub async fn command(program: &str, args: &[&str]) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = timeout(SYSTEM_COMMAND_TIMEOUT, command.output())
        .await
        .with_context(|| format!("timed out executing {program} {}", args.join(" ")))??;
    if !output.status.success() {
        bail!("{} {} failed: {}", program, args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

pub async fn clean_stale_resources() {
    let my_pid = std::process::id();
    if let Ok(mut entries) = tokio::fs::read_dir("/proc").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue; };
            if pid == my_pid { continue; }
            if let Ok(cmdline) = tokio::fs::read_to_string(format!("/proc/{pid}/cmdline")).await {
                if cmdline.contains("ssh-rocket-helper") {
                    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                }
            }
        }
    }
    let _ = command("nft", &["delete", "table", "inet", NFT_TABLE]).await;
    let _ = command("resolvectl", &["revert", TUN_NAME]).await;
    let _ = command("ip", &["-4", "rule", "del", "priority", DNS_RULE_PRIORITY]).await;
    let _ = command("ip", &["-4", "rule", "del", "priority", "21329"]).await;
    let _ = command("ip", &["-4", "rule", "del", "priority", TABLE]).await;
    let _ = command("ip", &["-6", "rule", "del", "priority", TABLE]).await;
    let _ = command("ip", &["-4", "route", "flush", "table", TABLE]).await;
    let _ = command("ip", &["-6", "route", "flush", "table", TABLE]).await;
    let _ = command("ip", &["-4", "route", "del", "10.0.0.1/32", "dev", TUN_NAME]).await;
    for group in CGROUPS {
        if let Ok(pids) = tokio::fs::read_to_string(format!("/sys/fs/cgroup/{group}/cgroup.procs")).await {
            for pid in pids.lines() {
                let _ = tokio::fs::write("/sys/fs/cgroup/cgroup.procs", pid).await;
            }
        }
        let _ = tokio::fs::remove_dir(format!("/sys/fs/cgroup/{group}")).await;
    }
}

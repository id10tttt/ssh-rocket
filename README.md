# SSH Rocket

SSH Rocket 是使用 Rust、GTK4 和 Libadwaita 重写的 Linux 原生 SSH 透明代理客户端。

```text
Application Traffic
        ↓
      Linux TUN
        ↓
Rust Routing Engine
  ├─ Custom Override
  ├─ App Rules
  ├─ Domain Rules
  ├─ IP/CIDR Rules
  └─ Default Policy
        ↓
DIRECT / SSH PROXY / BLOCK
```

本版本不兼容旧版 Vala 配置。新配置位于 `~/.config/ssh-rocket/config.json`。

## Workspace

- `ssh-rocket-core`：配置模型、规则优先级和路由决策。
- `ssh-rocket-runtime`：OpenSSH 会话、TUN 数据面、nftables/cgroup 策略和特权 helper。
- `ssh-rocket`：GTK4/Libadwaita 原生桌面界面。

规则优先级固定为：

```text
Custom Override > App Rule > Domain Rule > IP/CIDR Rule > Default Policy
```

TUN 到 SOCKS5 的用户态网络栈使用 Rust `tun2proxy` crate。SSH 传输继续使用系统 OpenSSH，因此保留 OpenSSH 的密钥、known_hosts 和 `~/.ssh/config` 能力。

## 构建与安装

Fedora：

```bash
sudo dnf install gcc gtk4-devel libadwaita-devel openssh-clients nftables iproute polkit
./build.sh
./install.sh
```

默认安装到 `/usr/local/bin/ssh-rocket` 和 `/usr/local/libexec/ssh-rocket-helper`。GUI 在连接时通过 Polkit 按需启动 helper，不再使用常驻 systemd helper。

开发运行：

```bash
cargo run -p ssh-rocket
```

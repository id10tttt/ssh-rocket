# SSH Rocket

基于 Vala、GTK4 和 Libadwaita 的原生 GNOME SSH 透明代理客户端。

SSH Rocket 使用 OpenSSH 建立本地 SOCKS5 和远端 TCP DNS 通道，由 tun2socks 将 TUN 中的 TCP 流量送入 SSH。远端只需运行允许 TCP 转发的 `sshd`，不需要安装额外程序。

## 功能

- SSH Agent、私钥和密码认证
- 全局或指定 CIDR 路由
- 按应用、域名和 IP 分流
- 从 HTTPS URL 或本地文件导入 Shadowrocket 规则，支持直连、代理和拒绝规则
- DNS 请求通过 SSH TCP 转发，代理规则失败时不回退到本地 DNS
- IPv4 和可选 IPv6 策略路由
- 黑名单应用阻断和 QUIC 降级
- 断线重连、实时日志和流量速率
- 退出后清理 TUN、nftables、策略路由和临时 cgroup

当前代理数据通道只支持 TCP。DNS 的 UDP 请求会在本机接收，再通过 SSH 中的 TCP 通道转发；普通 UDP、ICMP 和游戏流量不会通过 SSH。

## 依赖

构建需要 GLib 2.70、GTK 4.10、Libadwaita 1.5、Vala、Meson、Ninja、JSON-GLib 和 Libsoup 3。运行需要 `ssh`、`sshpass`（密码认证时）、`tun2socks`、`nft`、`ip`、`pkexec` 和桌面 Polkit 认证代理。

安装 xjasonlyu/tun2socks 2.6.0：

```bash
mkdir -p "$HOME/.local/bin"
GOBIN="$HOME/.local/bin" go install github.com/xjasonlyu/tun2socks/v2@v2.6.0
```

确保 `$HOME/.local/bin` 位于 `PATH`，再安装发行版提供的其余依赖。例如 Fedora：

```bash
sudo dnf install meson ninja-build gcc vala gtk4-devel libadwaita-devel json-glib-devel libsoup3-devel openssh-clients sshpass nftables iproute polkit
```

## 构建、安装与运行

首次安装或更新后，以普通桌面用户执行：

```bash
./run-gui.sh --install
```

脚本以普通用户编译，通过 sudo 安装程序、特权 helper、Polkit 策略、图标和桌面入口。程序默认安装到 `/usr/local`。更新 helper 代码后需要重新安装。

安装后可从应用菜单打开 **SSH Rocket**，或执行：

```bash
/usr/local/bin/ssh-rocket
```

开发运行：

```bash
./run-gui.sh
```

手动构建安装：

```bash
meson setup /tmp/ssh-rocket-build . --prefix=/usr/local
meson compile -C /tmp/ssh-rocket-build
sudo meson install -C /tmp/ssh-rocket-build --no-rebuild
```

界面、配置、OpenSSH 和 tun2socks 以当前桌面用户身份运行。特权 helper 只负责创建并清理 TUN、nftables、策略路由和 cgroup；每次应用会话首次连接时由 Polkit 请求授权。

## 快捷键

- `Ctrl + N`：新建 Profile
- `Ctrl + L`：打开日志窗口
- `Ctrl + Q`：退出程序

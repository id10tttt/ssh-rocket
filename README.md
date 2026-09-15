# SShuttle GUI

基于 **Vala + GTK4 + Libadwaita + Meson** 的原生 GNOME `sshuttle` 代理客户端（参考 `g4music` 架构）。

## 特性

- **纯原生体验**：采用 Vala 编译为本地 ELF 二进制机器码，启动极快，内存占用低，彻底摆脱 Python 解释器与 Conda/virtualenv 动态库（`libstdc++.so`）冲突。
- **现代化 GNOME HIG 界面**：
  - 简洁直观的连接状态总览与大按钮切换（Connect / Disconnect / Reconnect / Spinner）；
  - Profile 列表管理（支持自定义 SSH 主机、端口、用户名、远程 CIDR 路由、排除网络网段、DNS 转发与 IPv6）；
  - 独立实时流式日志窗口（支持一键复制到剪贴板与清空）。
- **标准 Meson 构建体系**：采用 GNOME 官方标准的 Meson + Ninja 构建与打包。

## 构建依赖

最低版本：GLib 2.70、GTK 4.10、Libadwaita 1.5。Vala 代码以 GLib 2.70 为目标生成。

对 Vala 自动生成的 C，仅定向忽略未使用变量、未使用函数和生成器的 const 限定符警告；
弃用 API、指针类型不兼容、返回值和分配大小检查保持开启。

在 Fedora / CentOS Stream / RHEL 上：

```bash
sudo dnf install -y meson ninja-build gcc vala gtk4-devel libadwaita-devel json-glib-devel sshuttle
```

在 Ubuntu / Debian 上：

```bash
sudo apt update && sudo apt install -y meson ninja-build valac libgtk-4-dev libadwaita-1-dev libjson-glib-dev sshuttle
```

在 Arch Linux / Manjaro 上：

```bash
sudo pacman -S meson ninja gcc vala gtk4 libadwaita json-glib sshuttle
```

## 构建、安装与运行

运行时需要系统已有的 `sshuttle`、`nft`、`pkexec` 和桌面 Polkit 认证代理。

首次安装或更新后，以普通用户执行：

```bash
./run-gui.sh --install
```

脚本以普通用户编译，通过 sudo 安装程序、后台 helper、Polkit 策略和 `.desktop` 应用菜单入口。
程序默认安装到 `/usr/local`，Polkit 策略安装到系统策略目录 `/usr/share/polkit-1/actions`。
构建文件保存在 `${XDG_CACHE_HOME:-$HOME/.cache}/sshuttle-gui/build`，不会使用仓库内已有的 `build/`。

安装后，从应用菜单打开 **SShuttle**，或执行：

```bash
/usr/local/bin/sshuttle-gui
```

开发时也可以通过 `./run-gui.sh` 编译并启动；更新 helper 代码后需重新安装。

界面、配置读写和 SSH 均使用当前桌面用户身份。首次连接时通过桌面认证窗口授权，
仅后台 helper 和 sshuttle 的路由运行时持有管理员权限。
同一次程序运行中的断线重连、手动断开再连接复用 helper，无需反复输入密码。
完整退出程序、helper 退出或重启电脑后，下次连接重新授权；不会配置永久免密 sudo。

helper 仅接受当前用户的私有连接，并限制可执行操作、进程归属和隧道参数。
正常断开时先等待 sshuttle 清理基础路由，再清理应用规则并恢复进程原来的 cgroup；
退出界面或连接丢失时，helper 同样执行清理后退出。

手动构建安装：

```bash
meson setup /tmp/sshuttle-gui-build . --prefix=/usr/local
meson compile -C /tmp/sshuttle-gui-build
sudo meson install -C /tmp/sshuttle-gui-build --no-rebuild
```

## 快捷键

- `Ctrl + N`: 新建 Profile
- `Ctrl + L`: 打开日志窗口
- `Ctrl + Q`: 退出程序

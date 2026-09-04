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

## 构建与运行

### 方式一：使用一键启动脚本（自动编译并运行）

```bash
./run-gui.sh
```

### 方式二：手动通过 Meson 构建

```bash
# 1. 初始化构建目录
meson setup build

# 2. 编译生成二进制程序
ninja -C build

# 3. 运行程序
./build/src/sshuttle-gui
```

### 方式三：系统级安装

```bash
sudo ninja -C build install
```

## 快捷键

- `Ctrl + N`: 新建 Profile
- `Ctrl + L`: 打开日志窗口
- `Ctrl + Q`: 退出程序

# SShuttle GUI

基于 **GTK4 + Libadwaita** 的 GNOME 原生 `sshuttle` 代理管理客户端。

## 特性

- **原生 GNOME 体验**：使用 GTK4 + Libadwaita 构建，严格契合 GNOME 设计规范（HIG）。
- **Profile 管理**：支持多主机配置切换，支持自定义端口、用户名、目标网络路由（CIDR）及排除网段。
- **状态机驱动**：清晰的连接生命周期（Disconnected / Connecting / Connected / Disconnecting / Error）。
- **安全解耦架构**：
  - 支持免提权用户态 GUI + Systemd / D-Bus 特权服务架构；
  - 同时内置直连回退模式，开箱即可通过 `pkexec` 本地运行测试。
- **实时日志流**：内置独立的日志查看器，支持实时流式追加、一键复制与清空。

## 运行依赖与环境检查

本项目是基于 PyGObject 的 GTK4/Libadwaita 原生应用，需要 Python 桥接包与系统原生 GUI 运行时库：

### 1. 快速检查当前环境

```bash
./check-env.py
```
该脚本会自动检测缺失的依赖，并给出当前系统的包安装建议。

### 2. 系统级依赖安装

- **Fedora / CentOS Stream / RHEL**：
  ```bash
  sudo dnf install -y python3-gobject gtk4 libadwaita sshuttle
  ```

- **Ubuntu / Debian**：
  ```bash
  sudo apt update && sudo apt install -y python3-gi python3-gi-cairo gir1.2-gtk-4.0 gir1.2-adw-1 sshuttle
  ```

- **Arch Linux / Manjaro**：
  ```bash
  sudo pacman -S python-gobject gtk4 libadwaita sshuttle
  ```

### 3. Python pip 依赖

若使用虚拟环境（venv）：

```bash
pip install -r requirements.txt
```
*(注意：虚拟环境依然需要系统安装有 gtk4 与 libadwaita 动态库及其 gobject-introspection typelib)*

## 本地运行

在项目根目录下直接执行：

```bash
./run-gui.sh
```

或：

```bash
python3 src/main.py
```

## 系统服务安装（生产环境推荐）

生产环境推荐使用常驻 systemd 服务以普通用户权限运行 GUI：

1. **安装 D-Bus 总线策略**：
   ```bash
   sudo cp data/io.giggle.Sshuttle.conf /usr/share/dbus-1/system.d/
   ```

2. **安装 Polkit 策略**：
   ```bash
   sudo cp data/io.giggle.SshuttleGUI.policy /usr/share/polkit-1/actions/
   ```

3. **安装可执行文件与 Systemd 服务**：
   ```bash
   sudo cp helper/daemon.py /usr/libexec/sshuttle-gui-helper
   sudo chmod +x /usr/libexec/sshuttle-gui-helper
   sudo cp systemd/sshuttle-gui.service /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable --now sshuttle-gui.service
   ```

## 快捷键

- `Ctrl + N`: 新建 Profile
- `Ctrl + L`: 打开日志窗口
- `Ctrl + Q`: 退出程序

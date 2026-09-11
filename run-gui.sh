#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"

if [ ! -d "${BUILD_DIR}" ]; then
    meson setup "${BUILD_DIR}" "${SCRIPT_DIR}"
fi
ninja -C "${BUILD_DIR}"

# 检查是否以 root 权限运行（管理 cgroup v2 与 nftables 规则需要 root 权限）
if [ "$(id -u)" -ne 0 ]; then
    echo "Running with sudo -E to preserve desktop environment and user credentials..."
    exec sudo -E "${BASH_SOURCE[0]}" "$@"
fi

# 检查并清理已有运行中的旧实例，避免多开端口冲突
OLD_PIDS=$(pgrep -f "${BUILD_DIR}/src/sshuttle-gui" || true)
if [ -n "${OLD_PIDS}" ]; then
    echo "Terminating existing SshuttleGUI instance(s): ${OLD_PIDS}..."
    kill -TERM ${OLD_PIDS} 2>/dev/null || true
    sleep 0.5
fi

exec "${BUILD_DIR}/src/sshuttle-gui" "$@"

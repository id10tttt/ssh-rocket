#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"

if [ ! -f "${BUILD_DIR}/src/sshuttle-gui" ]; then
    if [ ! -d "${BUILD_DIR}" ]; then
        meson setup "${BUILD_DIR}" "${SCRIPT_DIR}"
    fi
    ninja -C "${BUILD_DIR}"
fi

# 检查是否以 root 权限运行（管理 cgroup v2 与 nftables 规则需要 root 权限）
if [ "$(id -u)" -ne 0 ]; then
    echo "Running with sudo -E to preserve desktop environment and user credentials..."
    exec sudo -E "${BASH_SOURCE[0]}" "$@"
fi

exec "${BUILD_DIR}/src/sshuttle-gui" "$@"

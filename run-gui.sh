#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${XDG_CACHE_HOME:-${HOME}/.cache}/ssh-rocket/build"

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this script as your desktop user. Administrator access is requested separately." >&2
    exit 1
fi

if [ ! -d "${BUILD_DIR}" ]; then
    meson setup "${BUILD_DIR}" "${SCRIPT_DIR}" --prefix=/usr/local
fi
meson compile -C "${BUILD_DIR}"

if [ "${1:-}" = "--install" ]; then
    sudo meson install -C "${BUILD_DIR}" --no-rebuild
    exit 0
fi

if [ ! -x /usr/local/libexec/ssh-rocket-helper ]; then
    echo "Install the runtime helper and application menu entry first: ./run-gui.sh --install" >&2
    exit 1
fi

exec "${BUILD_DIR}/src/ssh-rocket" "$@"

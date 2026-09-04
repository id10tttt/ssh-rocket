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

exec "${BUILD_DIR}/src/sshuttle-gui" "$@"

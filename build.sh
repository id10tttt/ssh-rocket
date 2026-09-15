#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_BUILD_DIR="${SCRIPT_DIR}/build"

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this script as your desktop user." >&2
    exit 1
fi

if [ ! -d "${PROJECT_BUILD_DIR}" ]; then
    meson setup "${PROJECT_BUILD_DIR}" "${SCRIPT_DIR}" --prefix=/usr/local
else
    meson setup --reconfigure "${PROJECT_BUILD_DIR}" "${SCRIPT_DIR}" --prefix=/usr/local
fi

meson compile -C "${PROJECT_BUILD_DIR}"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_BUILD_DIR="${SCRIPT_DIR}/build"

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this script as your desktop user. Administrator access is requested separately." >&2
    exit 1
fi

if [ ! -f "${PROJECT_BUILD_DIR}/meson-private/coredata.dat" ]; then
    echo "No compiled build found. Run ./build.sh first." >&2
    exit 1
fi

sudo meson install -C "${PROJECT_BUILD_DIR}" --no-rebuild

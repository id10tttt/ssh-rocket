#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_BUILD_DIR="${SCRIPT_DIR}/build"

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this script as your desktop user. Administrator access is requested separately." >&2
    exit 1
fi

if [ ! -x "${PROJECT_BUILD_DIR}/src/ssh-rocket" ]; then
    echo "No compiled application found. Run ./build.sh first." >&2
    exit 1
fi

if [ ! -x /usr/local/libexec/ssh-rocket-helper ]; then
    echo "Install the runtime helper first: ./install.sh" >&2
    exit 1
fi

exec "${PROJECT_BUILD_DIR}/src/ssh-rocket" "$@"

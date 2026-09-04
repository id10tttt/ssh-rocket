#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PYTHONPATH="${SCRIPT_DIR}:${PYTHONPATH}"

# 检查依赖环境
python3 "${SCRIPT_DIR}/check-env.py" > /dev/null 2>&1 || {
    python3 "${SCRIPT_DIR}/check-env.py"
    exit 1
}

exec python3 "${SCRIPT_DIR}/src/main.py" "$@"

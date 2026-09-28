#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

PREFIX="${PREFIX:-/usr/local}"
DATA_PREFIX="${DATA_PREFIX:-/usr/share}"

# 1. 编译前端静态资源到 dist 目录
pnpm --prefix crates/ssh-rocket-tauri/ui build

# 2. Release 模式编译 Tauri 二进制与特权 helper
# Release 模式会将 dist 静态页面直接嵌入二进制中，无需本地 Vite 开发端口
cargo build --release -p ssh-rocket-tauri -p ssh-rocket-runtime

# 3. 安装可执行文件与特权 helper
sudo install -Dm755 target/release/ssh-rocket-tauri "$PREFIX/bin/ssh-rocket"
sudo install -Dm755 target/release/ssh-rocket-helper "$PREFIX/libexec/ssh-rocket-helper"

sed "s|@GUI_EXECUTABLE@|$PREFIX/bin/ssh-rocket|g" data/io.github.idi0t.SshRocket.desktop \
  | sudo tee "$DATA_PREFIX/applications/io.github.idi0t.SshRocket.desktop" >/dev/null
sed "s|@HELPER_PATH@|$PREFIX/libexec/ssh-rocket-helper|g" data/io.github.idi0t.SshRocket.policy \
  | sudo tee "$DATA_PREFIX/polkit-1/actions/io.github.idi0t.SshRocket.policy" >/dev/null

sudo install -Dm644 data/io.github.idi0t.SshRocket.metainfo.xml \
  "$DATA_PREFIX/metainfo/io.github.idi0t.SshRocket.metainfo.xml"
sudo install -Dm644 data/icons/ssh-rocket.svg \
  "$DATA_PREFIX/icons/hicolor/scalable/apps/ssh-rocket.svg"
sudo install -Dm644 data/icons/ssh-rocket-symbolic.svg \
  "$DATA_PREFIX/icons/hicolor/symbolic/apps/ssh-rocket-symbolic.svg"
for icon in disconnected acquiring connect rules traffic logs; do
  sudo install -Dm644 "data/icons/ssh-rocket-${icon}-symbolic.svg" \
    "$DATA_PREFIX/icons/hicolor/symbolic/apps/ssh-rocket-${icon}-symbolic.svg"
done

command -v update-desktop-database >/dev/null 2>&1 && sudo update-desktop-database "$DATA_PREFIX/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && sudo gtk-update-icon-cache -q -t -f "$DATA_PREFIX/icons/hicolor" || true

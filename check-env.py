#!/usr/bin/env python3
from __future__ import annotations

import os
from pathlib import Path
import shutil
import sys
from typing import List, Tuple


def detect_distro() -> str:
    """检测当前的 Linux 发行版类型"""
    os_release = Path("/etc/os-release")
    if os_release.exists():
        try:
            with open(os_release, "r", encoding="utf-8") as f:
                content = f.read().lower()
                if "fedora" in content or "rhel" in content or "centos" in content:
                    return "fedora"
                elif "ubuntu" in content or "debian" in content:
                    return "debian"
                elif "arch" in content or "manjaro" in content:
                    return "arch"
        except Exception:
            pass
    return "unknown"


def check_environment() -> Tuple[bool, List[str]]:
    """检查当前运行环境的依赖完整性"""
    missing = []

    # 1. 检查 Python 版本
    if sys.version_info < (3, 9):
        missing.append(f"Python 3.9+ required (current: {sys.version.split()[0]})")

    # 2. 检查 PyGObject
    try:
        import gi
    except ImportError:
        missing.append("PyGObject (python package)")
        gi = None

    # 3. 检查 GTK4
    if gi:
        try:
            gi.require_version("Gtk", "4.0")
            from gi.repository import Gtk
        except (ValueError, ImportError):
            missing.append("GTK 4.0 typelib / library")

    # 4. 检查 Libadwaita
    if gi:
        try:
            gi.require_version("Adw", "1")
            from gi.repository import Adw
        except (ValueError, ImportError):
            missing.append("Libadwaita (Adw 1.x) typelib / library")

    # 5. 检查 sshuttle 命令行工具
    if not shutil.which("sshuttle"):
        missing.append("sshuttle (binary)")

    return len(missing) == 0, missing


def main() -> int:
    """执行环境检测并输出指引"""
    ok, missing = check_environment()
    if ok:
        print("✓ All dependencies are installed and ready!")
        return 0

    print("✗ Missing dependencies detected:")
    for item in missing:
        print(f"  - {item}")

    distro = detect_distro()
    print("\nSuggested installation commands:")
    if distro == "fedora":
        print("  sudo dnf install -y python3-gobject gtk4 libadwaita sshuttle")
    elif distro == "debian":
        print("  sudo apt update && sudo apt install -y python3-gi python3-gi-cairo gir1.2-gtk-4.0 gir1.2-adw-1 sshuttle")
    elif distro == "arch":
        print("  sudo pacman -S python-gobject gtk4 libadwaita sshuttle")
    else:
        print("  Install PyGObject via pip: pip install -r requirements.txt")
        print("  Install system libraries: GTK4, Libadwaita, and sshuttle via your package manager.")

    return 1


if __name__ == "__main__":
    sys.exit(main())

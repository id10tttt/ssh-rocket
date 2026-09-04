#!/usr/bin/env python3
from __future__ import annotations

import os
import sys

# 确保将项目根目录加入到 sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw

from src.application import SshuttleApplication


def main() -> int:
    """主程序入口"""
    app = SshuttleApplication()
    return app.run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())

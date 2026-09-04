from __future__ import annotations

from typing import List
from .models import Profile


class CommandBuilder:
    """sshuttle 命令行执行参数构建器"""

    @staticmethod
    def build_argv(profile: Profile, use_pkexec: bool = False) -> List[str]:
        """根据 Profile 配置生成安全的命令行参数列表（避免 Shell 注入）"""
        if not profile.host.strip():
            raise ValueError("Host 不能为空")

        argv: List[str] = []
        if use_pkexec:
            argv.append("pkexec")

        argv.append("sshuttle")

        if profile.dns:
            argv.append("--dns")

        if profile.ipv6:
            argv.append("--ipv6")

        if profile.method and profile.method != "auto":
            argv.extend(["--method", profile.method])

        if profile.verbosity == "verbose":
            argv.append("-v")
        elif profile.verbosity == "very_verbose":
            argv.append("-vv")

        # 构造 SSH 目标 -r 参数
        argv.extend(["-r", profile.get_ssh_target()])

        # 排除网络网段
        for exc in profile.exclude:
            exc_clean = exc.strip()
            if exc_clean:
                argv.extend(["-x", exc_clean])

        # 远程目标路由
        routes = [r.strip() for r in profile.routes if r.strip()]
        if not routes:
            routes = ["0.0.0.0/0"]

        argv.extend(routes)
        return argv

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from enum import Enum
import uuid


class TunnelState(str, Enum):
    """sshuttle 隧道运行状态枚举"""
    DISCONNECTED = "disconnected"
    CONNECTING = "connecting"
    CONNECTED = "connected"
    DISCONNECTING = "disconnecting"
    ERROR = "error"


@dataclass
class Profile:
    """sshuttle 隧道配置模型"""
    id: str = field(default_factory=lambda: str(uuid.uuid4()))
    name: str = "Default"
    host: str = ""
    port: int = 22
    username: str = ""
    routes: list[str] = field(default_factory=lambda: ["0.0.0.0/0"])
    exclude: list[str] = field(default_factory=list)
    dns: bool = True
    ipv6: bool = False
    method: str = "auto"
    verbosity: str = "normal"
    auto_connect: bool = False

    def to_dict(self) -> dict:
        """序列化为字典对象"""
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> Profile:
        """从字典对象解析构建 Profile 实例"""
        return cls(
            id=data.get("id") or str(uuid.uuid4()),
            name=data.get("name", "Unnamed"),
            host=data.get("host", ""),
            port=int(data.get("port", 22)),
            username=data.get("username", ""),
            routes=list(data.get("routes", ["0.0.0.0/0"])),
            exclude=list(data.get("exclude", [])),
            dns=bool(data.get("dns", True)),
            ipv6=bool(data.get("ipv6", False)),
            method=str(data.get("method", "auto")),
            verbosity=str(data.get("verbosity", "normal")),
            auto_connect=bool(data.get("auto_connect", False)),
        )

    def get_ssh_target(self) -> str:
        """获取用于 sshuttle -r 参数的 SSH 目标地址"""
        target = self.host
        if self.username:
            target = f"{self.username}@{target}"
        if self.port and self.port != 22:
            target = f"{target}:{self.port}"
        return target

    def get_summary(self) -> str:
        """获取路由与关键特性的简要文本表示"""
        parts = []
        if self.routes:
            parts.append(", ".join(self.routes))
        if self.dns:
            parts.append("DNS")
        if self.ipv6:
            parts.append("IPv6")
        if self.exclude:
            parts.append(f"Exclude {len(self.exclude)}")
        return " · ".join(parts) if parts else "No routes"

from __future__ import annotations

import json
import os
from pathlib import Path
import tempfile
from typing import Any, Dict, List, Optional
from .models import Profile


class ConfigManager:
    """配置文件与存储管理器，负责 Profile 与应用状态的本地持久化"""

    def __init__(self, config_dir: Optional[Path] = None) -> None:
        if config_dir is None:
            env_dir = os.environ.get("SSHUTTLE_CONFIG_DIR")
            if env_dir:
                self.config_dir = Path(env_dir)
            else:
                self.config_dir = Path.home() / ".config" / "sshuttle-gui"
        else:
            self.config_dir = config_dir

        self.profiles_path = self.config_dir / "profiles.json"
        self.settings_path = self.config_dir / "settings.json"

        self._ensure_config_dir()
        self._profiles: List[Profile] = []
        self._active_profile_id: Optional[str] = None
        self._settings: Dict[str, Any] = {}
        self.load()

    def _ensure_config_dir(self) -> None:
        """确保配置目录存在，若宿主主目录为只读则降级到备选临时目录"""
        try:
            self.config_dir.mkdir(parents=True, exist_ok=True)
        except OSError:
            fallback = Path(tempfile.gettempdir()) / "sshuttle-gui"
            try:
                fallback.mkdir(parents=True, exist_ok=True)
                self.config_dir = fallback
                self.profiles_path = self.config_dir / "profiles.json"
                self.settings_path = self.config_dir / "settings.json"
            except Exception:
                pass

    def _atomic_write_json(self, target_path: Path, data: Any) -> None:
        """原子写入 JSON 文件，避免意外中断损坏配置文件"""
        self._ensure_config_dir()
        temp_file = None
        try:
            with tempfile.NamedTemporaryFile("w", dir=self.config_dir, delete=False, encoding="utf-8") as f:
                temp_file = Path(f.name)
                json.dump(data, f, indent=2, ensure_ascii=False)
            temp_file.replace(target_path)
        except Exception:
            if temp_file and temp_file.exists():
                temp_file.unlink(missing_ok=True)
            raise

    def load(self) -> None:
        """从磁盘加载配置文件"""
        if self.profiles_path.exists():
            try:
                with open(self.profiles_path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    self._profiles = [Profile.from_dict(item) for item in data.get("profiles", [])]
                    self._active_profile_id = data.get("active_profile_id")
            except Exception:
                self._profiles = []
                self._active_profile_id = None
        else:
            # 首次运行提供默认演示配置
            default_profile = Profile(
                name="Example VPS",
                host="gateway.example.com",
                port=22,
                username="root",
                routes=["0.0.0.0/0"],
                exclude=["192.168.0.0/16", "10.0.0.0/8"],
                dns=True,
                ipv6=False,
                method="auto",
            )
            self._profiles = [default_profile]
            self._active_profile_id = default_profile.id
            try:
                self.save_profiles()
            except Exception:
                pass

        if self.settings_path.exists():
            try:
                with open(self.settings_path, "r", encoding="utf-8") as f:
                    self._settings = json.load(f)
            except Exception:
                self._settings = {}
        else:
            self._settings = {
                "window_width": 460,
                "window_height": 680,
                "backend_mode": "auto",
            }
            try:
                self._atomic_write_json(self.settings_path, self._settings)
            except Exception:
                pass

    def save_profiles(self) -> None:
        """持久化保存所有 Profile 列表"""
        payload = {
            "active_profile_id": self._active_profile_id,
            "profiles": [p.to_dict() for p in self._profiles],
        }
        self._atomic_write_json(self.profiles_path, payload)

    def get_profiles(self) -> List[Profile]:
        """获取全部 Profile 列表"""
        return list(self._profiles)

    def get_profile_by_id(self, profile_id: str) -> Optional[Profile]:
        """根据 ID 查找指定 Profile"""
        for p in self._profiles:
            if p.id == profile_id:
                return p
        return None

    def get_active_profile(self) -> Optional[Profile]:
        """获取当前选中的活跃 Profile"""
        if self._active_profile_id:
            profile = self.get_profile_by_id(self._active_profile_id)
            if profile:
                return profile
        if self._profiles:
            return self._profiles[0]
        return None

    def set_active_profile(self, profile_id: str) -> None:
        """设置当前活跃的 Profile"""
        self._active_profile_id = profile_id
        try:
            self.save_profiles()
        except Exception:
            pass

    def save_profile(self, profile: Profile) -> None:
        """保存或更新单个 Profile"""
        found = False
        for i, existing in enumerate(self._profiles):
            if existing.id == profile.id:
                self._profiles[i] = profile
                found = True
                break
        if not found:
            self._profiles.append(profile)
        if not self._active_profile_id:
            self._active_profile_id = profile.id
        try:
            self.save_profiles()
        except Exception:
            pass

    def delete_profile(self, profile_id: str) -> bool:
        """删除指定 Profile"""
        initial_len = len(self._profiles)
        self._profiles = [p for p in self._profiles if p.id != profile_id]
        if len(self._profiles) < initial_len:
            if self._active_profile_id == profile_id:
                self._active_profile_id = self._profiles[0].id if self._profiles else None
            try:
                self.save_profiles()
            except Exception:
                pass
            return True
        return False

    def get_setting(self, key: str, default: Any = None) -> Any:
        """读取应用偏好设置"""
        return self._settings.get(key, default)

    def set_setting(self, key: str, value: Any) -> None:
        """保存应用偏好设置"""
        self._settings[key] = value
        try:
            self._atomic_write_json(self.settings_path, self._settings)
        except Exception:
            pass

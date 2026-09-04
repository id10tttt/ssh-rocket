from __future__ import annotations

import os
from pathlib import Path
import tempfile
import unittest

from src.command_builder import CommandBuilder
from src.config_manager import ConfigManager
from src.models import Profile, TunnelState


class TestSshuttleCore(unittest.TestCase):
    """核心模型、命令构建与存储管理器测试用例"""

    def test_profile_serialization(self) -> None:
        """测试 Profile 模型的序列化与反序列化"""
        p = Profile(
            name="Test VPS",
            host="1.2.3.4",
            port=2222,
            username="testuser",
            routes=["10.0.0.0/8", "192.168.1.0/24"],
            exclude=["10.1.0.0/16"],
            dns=True,
            ipv6=False,
            method="auto",
        )
        self.assertEqual(p.get_ssh_target(), "testuser@1.2.3.4:2222")
        summary = p.get_summary()
        self.assertIn("10.0.0.0/8", summary)
        self.assertIn("DNS", summary)
        self.assertIn("Exclude 1", summary)

        data = p.to_dict()
        p_loaded = Profile.from_dict(data)
        self.assertEqual(p_loaded.name, "Test VPS")
        self.assertEqual(p_loaded.host, "1.2.3.4")
        self.assertEqual(p_loaded.port, 2222)
        self.assertEqual(p_loaded.routes, ["10.0.0.0/8", "192.168.1.0/24"])

    def test_command_builder(self) -> None:
        """测试各种场景下 sshuttle 命令行参数构建"""
        p = Profile(
            name="HK",
            host="gateway.example.com",
            port=22,
            username="root",
            routes=["0.0.0.0/0"],
            exclude=["192.168.0.0/16", "10.0.0.0/8"],
            dns=True,
            ipv6=True,
            method="tproxy",
            verbosity="verbose",
        )

        argv = CommandBuilder.build_argv(p, use_pkexec=False)
        expected = [
            "sshuttle",
            "--dns",
            "--ipv6",
            "--method",
            "tproxy",
            "-v",
            "-r",
            "root@gateway.example.com",
            "-x",
            "192.168.0.0/16",
            "-x",
            "10.0.0.0/8",
            "0.0.0.0/0",
        ]
        self.assertEqual(argv, expected)

        # 测试 pkexec 前缀
        argv_pk = CommandBuilder.build_argv(p, use_pkexec=True)
        self.assertEqual(argv_pk[0], "pkexec")
        self.assertEqual(argv_pk[1:], expected)

    def test_command_builder_empty_host(self) -> None:
        """测试空 Host 参数报错"""
        p = Profile(host="")
        with self.assertRaises(ValueError):
            CommandBuilder.build_argv(p)

    def test_config_manager(self) -> None:
        """测试 ConfigManager 本地持久化与 CRUD"""
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            cm = ConfigManager(config_dir=temp_path)

            # 首次初始化应有默认 Profile
            profiles = cm.get_profiles()
            self.assertEqual(len(profiles), 1)

            # 新增 Profile
            new_p = Profile(name="New Node", host="8.8.8.8")
            cm.save_profile(new_p)
            self.assertEqual(len(cm.get_profiles()), 2)

            # 设置活跃 Profile
            cm.set_active_profile(new_p.id)
            active = cm.get_active_profile()
            self.assertIsNotNone(active)
            self.assertEqual(active.id, new_p.id)

            # 重新实例化验证磁盘加载一致性
            cm2 = ConfigManager(config_dir=temp_path)
            self.assertEqual(len(cm2.get_profiles()), 2)
            self.assertEqual(cm2.get_active_profile().id, new_p.id)

            # 删除 Profile
            cm2.delete_profile(new_p.id)
            self.assertEqual(len(cm2.get_profiles()), 1)


if __name__ == "__main__":
    unittest.main()

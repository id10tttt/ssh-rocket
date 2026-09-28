use gtk4::glib;
use std::path::{Path, PathBuf};

/// 已发现的 SSH 私钥信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredKey {
    /// 密钥展示名称（一般为文件名）
    pub name: String,
    /// 密钥文件完整路径
    pub path: PathBuf,
}

/// 获取用户默认 SSH 目录（通常为 ~/.ssh）
pub fn ssh_dir() -> Option<PathBuf> {
    let dir = glib::home_dir().join(".ssh");
    if dir.exists() {
        Some(dir)
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh"))
    }
}

/// 检查给定文件是否可能是 SSH 私钥
pub fn is_ssh_private_key(path: &Path, filename: &str) -> bool {
    let lower = filename.to_lowercase();
    if lower.ends_with(".pub")
        || lower.ends_with(".old")
        || lower.ends_with(".bak")
        || lower.ends_with(".tmp")
        || lower.ends_with(".swp")
        || lower.starts_with('.')
    {
        return false;
    }

    match lower.as_str() {
        "config" | "known_hosts" | "authorized_keys" | "authorized_keys2" | "environment" | "rc" => {
            return false;
        }
        _ => {}
    }

    // 尝试读取文件头部内容识别私钥标识
    if let Ok(content) = std::fs::read(path) {
        let prefix_len = content.len().min(512);
        let prefix = &content[..prefix_len];
        if let Ok(text) = std::str::from_utf8(prefix) {
            if text.contains("-----BEGIN ") || text.contains("PuTTY-User-Key-File") {
                return true;
            }
        }
    }

    // 针对标准命名私钥兜底（例如权限无法读取等情况）
    matches!(
        lower.as_str(),
        "id_rsa" | "id_ed25519" | "id_ecdsa" | "id_dsa"
    )
}

/// 扫描 ~/.ssh 目录中的所有有效私钥，并按优先级排序返回
pub fn scan_ssh_keys() -> Vec<DiscoveredKey> {
    let Some(dir) = ssh_dir() else {
        return Vec::new();
    };

    if !dir.is_dir() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut keys = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if is_ssh_private_key(&path, name) {
            keys.push(DiscoveredKey {
                name: name.to_string(),
                path,
            });
        }
    }

    // 常用标准密钥排在前面，其余按名称字母升序排列
    keys.sort_by(|a, b| {
        let rank = |name: &str| match name {
            "id_ed25519" => 0,
            "id_rsa" => 1,
            "id_ecdsa" => 2,
            "id_dsa" => 3,
            _ => 10,
        };
        let rank_a = rank(&a.name);
        let rank_b = rank(&b.name);
        if rank_a != rank_b {
            rank_a.cmp(&rank_b)
        } else {
            a.name.cmp(&b.name)
        }
    });

    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ssh_private_key_filter() {
        let dummy = Path::new("/tmp/test");
        assert!(!is_ssh_private_key(dummy, "id_rsa.pub"));
        assert!(!is_ssh_private_key(dummy, "known_hosts"));
        assert!(!is_ssh_private_key(dummy, "config"));
        assert!(!is_ssh_private_key(dummy, ".hidden"));
        assert!(is_ssh_private_key(dummy, "id_rsa"));
        assert!(is_ssh_private_key(dummy, "id_ed25519"));
    }

    #[test]
    fn test_scan_ssh_keys() {
        let keys = scan_ssh_keys();
        // 如果本地存在 ~/.ssh，应该能扫描出私钥
        if let Some(dir) = ssh_dir() {
            if dir.is_dir() {
                assert!(!keys.is_empty(), "Should discover ssh keys in user .ssh directory");
                // 确保没有误把 .pub 或 config 扫进去
                for key in &keys {
                    assert!(!key.name.ends_with(".pub"));
                    assert_ne!(key.name, "config");
                    assert_ne!(key.name, "known_hosts");
                }
            }
        }
    }
}

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{fs, net::IpAddr, path::PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot determine config directory")]
    NoConfigDirectory,
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),
    #[error("invalid config: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Direct,
    #[default]
    Proxy,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    #[default]
    Key,
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub auth_type: AuthType,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
}

fn default_ssh_port() -> u16 {
    22
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "New Server".into(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: AuthType::Key,
            password: None,
            identity_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRule {
    pub executable: PathBuf,
    pub action: RuleAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainRule {
    pub pattern: String,
    pub action: RuleAction,
    #[serde(default)]
    pub kind: DomainRuleKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DomainRuleKind {
    #[default]
    Legacy,
    Domain,
    DomainSuffix,
    DomainKeyword,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(default)]
    pub default_policy: RuleAction,
    #[serde(default)]
    pub custom_overrides: Vec<IpRule>,
    #[serde(default)]
    pub app_rules: Vec<AppRule>,
    #[serde(default)]
    pub domain_rules: Vec<DomainRule>,
    #[serde(default)]
    pub imported_domain_rules: Vec<DomainRule>,
    #[serde(default)]
    pub ip_rules: Vec<IpRule>,
    #[serde(default)]
    pub imported_ip_rules: Vec<IpRule>,
    #[serde(default)]
    pub rule_source_url: String,
    #[serde(default)]
    pub rule_source_name: String,
    #[serde(default)]
    pub rule_source_updated_at: i64,
    #[serde(default = "default_dns")]
    pub dns_server: IpAddr,
    #[serde(default)]
    pub ipv6: bool,
}

fn default_dns() -> IpAddr {
    "1.1.1.1".parse().expect("valid default DNS")
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            default_policy: RuleAction::Proxy,
            custom_overrides: Vec::new(),
            app_rules: Vec::new(),
            domain_rules: Vec::new(),
            imported_domain_rules: Vec::new(),
            ip_rules: Vec::new(),
            imported_ip_rules: Vec::new(),
            rule_source_url: String::new(),
            rule_source_name: String::new(),
            rule_source_updated_at: 0,
            dns_server: default_dns(),
            ipv6: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRule {
    pub network: IpNet,
    pub action: RuleAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub active_profile: Option<Uuid>,
    #[serde(default)]
    pub settings: GlobalSettings,
}

impl AppConfig {
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|path| path.join("ssh-rocket").join("config.json"))
            .ok_or(ConfigError::NoConfigDirectory)
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        let active = self.active_profile?;
        self.profiles.iter().find(|profile| profile.id == active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_backward_compatibility() {
        let old_json = r#"{
            "id": "a0000000-0000-0000-0000-000000000001",
            "name": "Old Server",
            "host": "192.168.1.1",
            "port": 22,
            "username": "root",
            "identity_file": "/home/user/.ssh/id_rsa"
        }"#;

        let profile: Profile = serde_json::from_str(old_json).expect("should deserialize old profile");
        assert_eq!(profile.auth_type, AuthType::Key);
        assert_eq!(profile.password, None);
        assert_eq!(profile.identity_file, Some(PathBuf::from("/home/user/.ssh/id_rsa")));
    }

    #[test]
    fn test_profile_password_auth_roundtrip() {
        let profile = Profile {
            id: Uuid::new_v4(),
            name: "Pwd Server".into(),
            host: "example.com".into(),
            port: 2222,
            username: "admin".into(),
            auth_type: AuthType::Password,
            password: Some("mypassword123".into()),
            identity_file: None,
        };

        let json = serde_json::to_string(&profile).expect("serialize");
        let decoded: Profile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.auth_type, AuthType::Password);
        assert_eq!(decoded.password.as_deref(), Some("mypassword123"));
        assert_eq!(decoded.identity_file, None);
    }
}


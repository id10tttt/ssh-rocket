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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Auto,
    Chinese,
    English,
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
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub floating_widget: FloatingWidgetConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatingWidgetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_idle_opacity")]
    pub idle_opacity: f64,
    #[serde(default = "default_fade_delay_secs")]
    pub fade_delay_secs: u32,
    #[serde(default = "default_speed_decimals")]
    pub speed_decimals: u32,
}

fn default_true() -> bool {
    true
}

fn default_idle_opacity() -> f64 {
    0.5
}

fn default_fade_delay_secs() -> u32 {
    5
}

fn default_speed_decimals() -> u32 {
    1
}

impl Default for FloatingWidgetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_opacity: default_idle_opacity(),
            fade_delay_secs: default_fade_delay_secs(),
            speed_decimals: default_speed_decimals(),
        }
    }
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
            theme_mode: ThemeMode::Auto,
            language: Language::Chinese,
            floating_widget: FloatingWidgetConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRule {
    pub network: IpNet,
    pub action: RuleAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ForwardType {
    #[default]
    Local,
    Remote,
}

pub fn default_forward_host() -> String {
    "127.0.0.1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardRule {
    pub id: Uuid,
    pub name: String,
    pub profile_id: Uuid,
    #[serde(default)]
    pub forward_type: ForwardType,
    #[serde(default = "default_forward_host")]
    pub local_host: String,
    pub local_port: u16,
    #[serde(default = "default_forward_host")]
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for PortForwardRule {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            profile_id: Uuid::nil(),
            forward_type: ForwardType::Local,
            local_host: default_forward_host(),
            local_port: 0,
            remote_host: default_forward_host(),
            remote_port: 0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub active_profile: Option<Uuid>,
    #[serde(default)]
    pub port_forwards: Vec<PortForwardRule>,
    #[serde(default)]
    pub settings: GlobalSettings,
}

impl AppConfig {
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|path| path.join("ssh-rocket").join("config.json"))
            .ok_or(ConfigError::NoConfigDirectory)
    }

    pub fn log_dir() -> Result<PathBuf, ConfigError> {
        dirs::data_dir()
            .map(|path| path.join("ssh-rocket").join("logs"))
            .ok_or(ConfigError::NoConfigDirectory)
    }

    pub fn log_file_path() -> Result<PathBuf, ConfigError> {
        Self::log_dir().map(|path| path.join("ssh-rocket.log"))
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

    #[test]
    fn test_settings_theme_and_language() {
        // 旧配置缺失字段时，默认应为 Auto 和 Auto
        let old_json = r#"{}"#;
        let settings: GlobalSettings = serde_json::from_str(old_json).expect("deserialize empty");
        assert_eq!(settings.theme_mode, ThemeMode::Auto);
        assert_eq!(settings.language, Language::Auto);

        // 正常往返序列化
        let mut s = GlobalSettings::default();
        s.theme_mode = ThemeMode::Dark;
        s.language = Language::English;
        let json = serde_json::to_string(&s).expect("serialize");
        let decoded: GlobalSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.theme_mode, ThemeMode::Dark);
        assert_eq!(decoded.language, Language::English);
    }

    #[test]
    fn test_floating_widget_config() {
        // 旧配置缺失 floating_widget 时应能正常解析且为默认值
        let old_json = r#"{}"#;
        let settings: GlobalSettings = serde_json::from_str(old_json).expect("deserialize empty");
        assert!(settings.floating_widget.enabled);
        assert!((settings.floating_widget.idle_opacity - 0.5).abs() < 1e-4);
        assert_eq!(settings.floating_widget.fade_delay_secs, 5);
        assert_eq!(settings.floating_widget.speed_decimals, 1);

        // 往返序列化
        let mut s = GlobalSettings::default();
        s.floating_widget.enabled = false;
        s.floating_widget.idle_opacity = 0.35;
        s.floating_widget.fade_delay_secs = 10;
        s.floating_widget.speed_decimals = 2;
        let json = serde_json::to_string(&s).expect("serialize");
        let decoded: GlobalSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(!decoded.floating_widget.enabled);
        assert!((decoded.floating_widget.idle_opacity - 0.35).abs() < 1e-4);
        assert_eq!(decoded.floating_widget.fade_delay_secs, 10);
        assert_eq!(decoded.floating_widget.speed_decimals, 2);
    }

    #[test]
    fn test_port_forward_rule_serialization() {
        let old_json = r#"{
            "profiles": [],
            "settings": {}
        }"#;
        let config: AppConfig = serde_json::from_str(old_json).expect("deserialize old config without port_forwards");
        assert!(config.port_forwards.is_empty());

        let rule = PortForwardRule {
            id: Uuid::new_v4(),
            name: "MySQL 映射".into(),
            profile_id: Uuid::new_v4(),
            forward_type: ForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port: 13306,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
            enabled: true,
        };
        let mut new_config = AppConfig::default();
        new_config.port_forwards.push(rule.clone());

        let json = serde_json::to_string(&new_config).expect("serialize config with port_forwards");
        let decoded: AppConfig = serde_json::from_str(&json).expect("deserialize config with port_forwards");
        assert_eq!(decoded.port_forwards.len(), 1);
        assert_eq!(decoded.port_forwards[0].name, "MySQL 映射");
        assert_eq!(decoded.port_forwards[0].forward_type, ForwardType::Local);
        assert_eq!(decoded.port_forwards[0].local_port, 13306);
        assert_eq!(decoded.port_forwards[0].remote_port, 3306);
    }
}

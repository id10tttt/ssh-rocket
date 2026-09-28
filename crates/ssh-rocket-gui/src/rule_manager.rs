use libadwaita as adw;
use ssh_rocket_core::{
    parse_rule_set, parse_shadowrocket_rules, AppConfig, DomainRule,
    DomainRuleKind, IpRule, RuleAction, RuleImportResult,
};
use std::process::Command as StdCommand;

pub const MAX_RULE_SOURCE_SIZE: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub enum ListedRule {
    Domain(DomainRule),
    Ip(IpRule),
}

impl ListedRule {
    pub fn value(&self) -> String {
        match self {
            Self::Domain(rule) => rule.pattern.clone(),
            Self::Ip(rule) => rule.network.to_string(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Domain(rule) => domain_kind_label(rule.kind),
            Self::Ip(_) => "IP-CIDR",
        }
    }

    pub fn action(&self) -> RuleAction {
        match self {
            Self::Domain(rule) => rule.action,
            Self::Ip(rule) => rule.action,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.value().to_lowercase().contains(query)
            || self.kind_label().to_lowercase().contains(query)
            || action_label(self.action()).to_lowercase().contains(query)
    }
}

#[derive(Default)]
pub struct RuleListState {
    pub filtered: Vec<ListedRule>,
    pub rendered_rows: Vec<adw::ActionRow>,
    pub loaded: usize,
}

pub fn action_label(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Direct => "DIRECT",
        RuleAction::Proxy => "PROXY",
        RuleAction::Block => "REJECT",
    }
}

pub fn domain_kind_label(kind: DomainRuleKind) -> &'static str {
    match kind {
        DomainRuleKind::Domain => "DOMAIN",
        DomainRuleKind::DomainSuffix => "DOMAIN-SUFFIX",
        DomainRuleKind::DomainKeyword => "DOMAIN-KEYWORD",
        DomainRuleKind::Legacy => "LEGACY",
    }
}

pub fn custom_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(config.settings.ip_rules.iter().cloned().map(ListedRule::Ip))
        .collect()
}

pub fn imported_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .imported_domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(
            config
                .settings
                .imported_ip_rules
                .iter()
                .cloned()
                .map(ListedRule::Ip),
        )
        .collect()
}

pub fn rule_source_name(source_url: &str) -> String {
    source_url
        .split(['?', '#'])
        .next()
        .and_then(|url| url.rsplit('/').find(|part| !part.is_empty()))
        .filter(|name| !name.is_empty())
        .unwrap_or("订阅规则")
        .to_string()
}

pub fn remove_listed_rule(config: &mut AppConfig, rule: &ListedRule) {
    match rule {
        ListedRule::Domain(rule) => config
            .settings
            .domain_rules
            .retain(|item| !(item.pattern == rule.pattern && item.kind == rule.kind)),
        ListedRule::Ip(rule) => config
            .settings
            .ip_rules
            .retain(|item| item.network != rule.network),
    }
}

pub fn import_rule_source(url: &str) -> Result<RuleImportResult, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let content = download_rule_text(url)?;
    let mut result = parse_shadowrocket_rules(&content);
    let rule_sets = result.rule_sets.clone();
    for reference in rule_sets.into_iter().take(8) {
        match download_rule_text(&reference.url) {
            Ok(content) => result.merge(parse_rule_set(&content, reference.action)),
            Err(error) => {
                result.ignored_count += 1;
                result.warnings.push(format!("子规则集跳过: {error}"));
            }
        }
    }
    if result.rule_sets.len() > 8 {
        result.ignored_count += result.rule_sets.len() - 8;
        result.warnings.push("部分超出数量限制的子规则集已被跳过".into());
    }
    if result.rule_count() == 0 {
        return Err("规则源不包含任何支持的有效规则".into());
    }
    Ok(result)
}

pub fn download_rule_text(url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let output = StdCommand::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "120",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            &MAX_RULE_SOURCE_SIZE.to_string(),
            url,
        ])
        .output()
        .map_err(|error| format!("curl 命令启动失败: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            format!("curl 进程退出码: {}", output.status)
        } else {
            error
        });
    }
    if output.stdout.len() > MAX_RULE_SOURCE_SIZE {
        return Err("规则源文件大小超过 16 MB 限制".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "规则内容非有效 UTF-8 编码".into())
}

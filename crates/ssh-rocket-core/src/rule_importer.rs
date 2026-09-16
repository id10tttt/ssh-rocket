use crate::{DomainRule, DomainRuleKind, IpRule, RuleAction};
use ipnet::IpNet;
use std::{collections::HashSet, net::IpAddr, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSetReference {
    pub url: String,
    pub action: RuleAction,
}

#[derive(Debug, Clone)]
pub struct RuleImportResult {
    pub domain_rules: Vec<DomainRule>,
    pub ip_rules: Vec<IpRule>,
    pub rule_sets: Vec<RuleSetReference>,
    pub warnings: Vec<String>,
    pub default_policy: RuleAction,
    pub ignored_count: usize,
}

impl Default for RuleImportResult {
    fn default() -> Self {
        Self {
            domain_rules: Vec::new(),
            ip_rules: Vec::new(),
            rule_sets: Vec::new(),
            warnings: Vec::new(),
            default_policy: RuleAction::Direct,
            ignored_count: 0,
        }
    }
}

impl RuleImportResult {
    pub fn rule_count(&self) -> usize {
        self.domain_rules.len() + self.ip_rules.len()
    }

    pub fn action_counts(&self) -> (usize, usize, usize) {
        let actions = self
            .domain_rules
            .iter()
            .map(|rule| rule.action)
            .chain(self.ip_rules.iter().map(|rule| rule.action));
        actions.fold((0, 0, 0), |(direct, proxy, block), action| match action {
            RuleAction::Direct => (direct + 1, proxy, block),
            RuleAction::Proxy => (direct, proxy + 1, block),
            RuleAction::Block => (direct, proxy, block + 1),
        })
    }

    pub fn merge(&mut self, other: RuleImportResult) {
        let mut domain_keys: HashSet<_> = self
            .domain_rules
            .iter()
            .map(|rule| (rule.kind, rule.pattern.clone()))
            .collect();
        for rule in other.domain_rules {
            if domain_keys.insert((rule.kind, rule.pattern.clone())) {
                self.domain_rules.push(rule);
            }
        }

        let mut ip_keys: HashSet<_> = self.ip_rules.iter().map(|rule| rule.network).collect();
        for rule in other.ip_rules {
            if ip_keys.insert(rule.network) {
                self.ip_rules.push(rule);
            }
        }
        self.ignored_count += other.ignored_count;
        for warning in other.warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }
}

/// 解析 Shadowrocket 配置中的 General 与 Rule 段。
pub fn parse_shadowrocket_rules(content: &str) -> RuleImportResult {
    let mut result = RuleImportResult::default();
    let mut section = "";
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line;
            continue;
        }
        match section.to_ascii_lowercase().as_str() {
            "[general]" => parse_general_line(line, &mut result),
            "[rule]" => parse_rule_line(line, None, true, &mut result),
            _ => {
                if !section.is_empty() {
                    result.ignored_count += 1;
                }
            }
        }
    }
    result
}

/// 解析 RULE-SET 下载到的规则列表，并将其动作统一为引用规则指定的动作。
pub fn parse_rule_set(content: &str, action: RuleAction) -> RuleImportResult {
    let mut result = RuleImportResult::default();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        parse_rule_line(line, Some(action), false, &mut result);
    }
    result
}

fn parse_general_line(line: &str, result: &mut RuleImportResult) {
    let Some((key, values)) = line.split_once('=') else {
        return;
    };
    if !matches!(key.trim().to_ascii_lowercase().as_str(), "skip-proxy" | "bypass-tun") {
        return;
    }
    for raw_value in values.split(',') {
        let value = raw_value.trim().trim_end_matches('.').to_ascii_lowercase();
        if value.is_empty() || value == "localhost" {
            continue;
        }
        if let Some(network) = parse_network(&value) {
            push_ip_rule(result, network, RuleAction::Direct);
        } else if let Some(suffix) = value.strip_prefix("*.") {
            push_domain_rule(result, suffix, RuleAction::Direct, DomainRuleKind::DomainSuffix);
        } else {
            push_domain_rule(result, &value, RuleAction::Direct, DomainRuleKind::Domain);
        }
    }
}

fn parse_rule_line(
    line: &str,
    inherited_action: Option<RuleAction>,
    collect_rule_sets: bool,
    result: &mut RuleImportResult,
) {
    let parts: Vec<_> = line.split(',').map(str::trim).collect();
    if parts.len() < 2 {
        result.ignored_count += 1;
        return;
    }
    let rule_type = parts[0].to_ascii_uppercase();
    let value = parts[1].trim();
    let action = inherited_action.unwrap_or_else(|| parts.get(2).map_or(RuleAction::Proxy, |value| normalize_action(value)));

    match rule_type.as_str() {
        "DOMAIN" => push_domain_rule(result, value, action, DomainRuleKind::Domain),
        "DOMAIN-SUFFIX" => push_domain_rule(result, value, action, DomainRuleKind::DomainSuffix),
        "DOMAIN-KEYWORD" => push_domain_rule(result, value, action, DomainRuleKind::DomainKeyword),
        "LEGACY" => push_domain_rule(result, value, action, DomainRuleKind::Legacy),
        "IP-CIDR" | "IP-CIDR6" => {
            if let Some(network) = parse_network(value) {
                push_ip_rule(result, network, action);
            } else {
                result.ignored_count += 1;
            }
        }
        "RULE-SET" => {
            if collect_rule_sets && value.starts_with("https://") {
                result.rule_sets.push(RuleSetReference { url: value.to_string(), action });
            } else {
                result.ignored_count += 1;
            }
        }
        "FINAL" => result.default_policy = normalize_action(value),
        _ => result.ignored_count += 1,
    }
}

fn normalize_action(value: &str) -> RuleAction {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("reject") || value == "block" {
        RuleAction::Block
    } else if value == "direct" {
        RuleAction::Direct
    } else {
        RuleAction::Proxy
    }
}

fn parse_network(value: &str) -> Option<IpNet> {
    if let Ok(network) = IpNet::from_str(value) {
        return Some(network);
    }
    let address = IpAddr::from_str(value).ok()?;
    IpNet::new(address, if address.is_ipv4() { 32 } else { 128 }).ok()
}

fn push_domain_rule(result: &mut RuleImportResult, value: &str, action: RuleAction, kind: DomainRuleKind) {
    let pattern = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if pattern.is_empty() {
        result.ignored_count += 1;
        return;
    }
    if result.domain_rules.iter().any(|rule| rule.kind == kind && rule.pattern == pattern) {
        return;
    }
    result.domain_rules.push(DomainRule { pattern, action, kind });
}

fn push_ip_rule(result: &mut RuleImportResult, network: IpNet, action: RuleAction) {
    if result.ip_rules.iter().any(|rule| rule.network == network) {
        return;
    }
    result.ip_rules.push(IpRule { network, action });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shadowrocket_rules() {
        let result = parse_shadowrocket_rules(
            "[General]\nskip-proxy = *.lan.test, 10.0.0.0/8\n[Rule]\nDOMAIN,api.example.com,DIRECT\nDOMAIN-SUFFIX,example.org,PROXY\nDOMAIN-KEYWORD,ads,REJECT\nIP-CIDR,192.0.2.0/24,DIRECT,no-resolve\nRULE-SET,https://example.com/rules.list,REJECT\nFINAL,PROXY\n",
        );
        assert_eq!(result.domain_rules.len(), 4);
        assert_eq!(result.ip_rules.len(), 2);
        assert_eq!(result.rule_sets.len(), 1);
        assert_eq!(result.default_policy, RuleAction::Proxy);
        assert_eq!(result.domain_rules[3].action, RuleAction::Block);
    }
}

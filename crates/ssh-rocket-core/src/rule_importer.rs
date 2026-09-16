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

/// 解析 ZeroOmega / SwitchyOmega 备份文件中的规则列表
pub fn parse_omega_rules(content: &str) -> Result<RuleImportResult, String> {
    let root: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "Invalid Omega backup: expected root object".to_string())?;

    let mut result = RuleImportResult::default();
    let mut found_switch_profile = false;

    for (key, val) in obj {
        if !key.starts_with('+') {
            continue;
        }
        let Some(profile) = val.as_object() else {
            continue;
        };
        let profile_type = profile
            .get("profileType")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if profile_type != "SwitchProfile" {
            continue;
        }
        found_switch_profile = true;
        let Some(rules) = profile.get("rules").and_then(|v| v.as_array()) else {
            continue;
        };

        for r in rules {
            let Some(r_obj) = r.as_object() else {
                result.ignored_count += 1;
                continue;
            };
            let profile_name = r_obj
                .get("profileName")
                .and_then(|v| v.as_str())
                .unwrap_or("proxy")
                .trim()
                .to_ascii_lowercase();
            let action = if profile_name == "direct" {
                RuleAction::Direct
            } else if profile_name == "reject" || profile_name == "block" {
                RuleAction::Block
            } else {
                RuleAction::Proxy
            };

            let Some(cond) = r_obj.get("condition").and_then(|v| v.as_object()) else {
                result.ignored_count += 1;
                continue;
            };
            let condition_type = cond
                .get("conditionType")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let pattern = cond
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();

            if pattern.is_empty() {
                result.ignored_count += 1;
                continue;
            }

            match condition_type {
                "HostWildcardCondition" => {
                    if let Some(net) = parse_wildcard_ip(pattern) {
                        push_ip_rule(&mut result, net, action);
                    } else if let Some(net) = parse_network(pattern) {
                        push_ip_rule(&mut result, net, action);
                    } else if pattern.starts_with("*.") && pattern.ends_with(".*") && pattern.len() > 4 {
                        let keyword = pattern.trim_matches('*').trim_matches('.');
                        push_domain_rule(&mut result, keyword, action, DomainRuleKind::DomainKeyword);
                    } else if pattern == "*.*.cn" || (pattern.starts_with("*.*.") && pattern.len() > 4) {
                        let suffix = format!(
                            "*.{}",
                            pattern
                                .trim_start_matches('*')
                                .trim_start_matches('.')
                                .trim_start_matches('*')
                                .trim_start_matches('.')
                        );
                        push_domain_rule(&mut result, &suffix, action, DomainRuleKind::DomainSuffix);
                    } else if pattern.starts_with("*.") {
                        push_domain_rule(&mut result, pattern, action, DomainRuleKind::DomainSuffix);
                    } else {
                        push_domain_rule(&mut result, pattern, action, DomainRuleKind::Domain);
                    }
                }
                "IpCondition" => {
                    if let Some(net) = parse_network(pattern) {
                        push_ip_rule(&mut result, net, action);
                    } else {
                        result.ignored_count += 1;
                    }
                }
                "BypassCondition" => {
                    let direct_action = RuleAction::Direct;
                    if let Some(net) = parse_wildcard_ip(pattern).or_else(|| parse_network(pattern)) {
                        push_ip_rule(&mut result, net, direct_action);
                    } else {
                        push_domain_rule(&mut result, pattern, direct_action, DomainRuleKind::Domain);
                    }
                }
                "UrlWildcardCondition" => {
                    if let Some(host) = extract_host_from_url_wildcard(pattern) {
                        if let Some(net) = parse_wildcard_ip(&host).or_else(|| parse_network(&host)) {
                            push_ip_rule(&mut result, net, action);
                        } else if host.starts_with("*.") {
                            push_domain_rule(&mut result, &host, action, DomainRuleKind::DomainSuffix);
                        } else {
                            push_domain_rule(&mut result, &host, action, DomainRuleKind::Domain);
                        }
                    } else {
                        result.ignored_count += 1;
                    }
                }
                _ => {
                    result.ignored_count += 1;
                }
            }
        }
    }

    if !found_switch_profile {
        return Err("No SwitchProfile found in Omega configuration".to_string());
    }
    if result.rule_count() == 0 {
        return Err("No supported rules found in SwitchProfile".to_string());
    }

    Ok(result)
}

fn parse_wildcard_ip(pattern: &str) -> Option<IpNet> {
    let parts: Vec<&str> = pattern.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    use std::net::Ipv4Addr;
    if parts[1] == "*" && parts[2] == "*" && parts[3] == "*" {
        let a = parts[0].parse::<u8>().ok()?;
        IpNet::new(IpAddr::V4(Ipv4Addr::new(a, 0, 0, 0)), 8).ok()
    } else if parts[2] == "*" && parts[3] == "*" {
        let a = parts[0].parse::<u8>().ok()?;
        let b = parts[1].parse::<u8>().ok()?;
        IpNet::new(IpAddr::V4(Ipv4Addr::new(a, b, 0, 0)), 16).ok()
    } else if parts[3] == "*" {
        let a = parts[0].parse::<u8>().ok()?;
        let b = parts[1].parse::<u8>().ok()?;
        let c = parts[2].parse::<u8>().ok()?;
        IpNet::new(IpAddr::V4(Ipv4Addr::new(a, b, c, 0)), 24).ok()
    } else {
        None
    }
}

fn extract_host_from_url_wildcard(url: &str) -> Option<String> {
    let without_scheme = if let Some(rest) = url.strip_prefix("*://") {
        rest
    } else if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        url
    };
    let host_part = without_scheme.split(['/', ':', '?']).next()?.trim();
    if host_part.is_empty() {
        None
    } else {
        Some(host_part.to_string())
    }
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

    #[test]
    fn parses_omega_rules_basic() {
        let json = r#"{
            "+auto switch": {
                "profileType": "SwitchProfile",
                "rules": [
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "*.example.com"
                        },
                        "profileName": "direct"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "172.16.*.*"
                        },
                        "profileName": "direct"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "10.0.0.*"
                        },
                        "profileName": "direct"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "1.2.3.4"
                        },
                        "profileName": "proxy"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "*.dingding.*"
                        },
                        "profileName": "direct"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "*.*.cn"
                        },
                        "profileName": "direct"
                    },
                    {
                        "condition": {
                            "conditionType": "HostWildcardCondition",
                            "pattern": "plaindomain.org"
                        },
                        "profileName": "proxy"
                    }
                ]
            }
        }"#;

        let result = parse_omega_rules(json).expect("should parse successfully");
        assert_eq!(result.ip_rules.len(), 3);
        assert_eq!(result.domain_rules.len(), 4);

        // Verify IP rules
        let ip16 = result.ip_rules.iter().find(|r| r.network.to_string() == "172.16.0.0/16");
        assert!(ip16.is_some());
        assert_eq!(ip16.unwrap().action, RuleAction::Direct);

        let ip24 = result.ip_rules.iter().find(|r| r.network.to_string() == "10.0.0.0/24");
        assert!(ip24.is_some());
        assert_eq!(ip24.unwrap().action, RuleAction::Direct);

        let ip32 = result.ip_rules.iter().find(|r| r.network.to_string() == "1.2.3.4/32");
        assert!(ip32.is_some());
        assert_eq!(ip32.unwrap().action, RuleAction::Proxy);

        // Verify Domain rules
        let suffix = result.domain_rules.iter().find(|r| r.pattern == "*.example.com");
        assert!(suffix.is_some());
        assert_eq!(suffix.unwrap().kind, DomainRuleKind::DomainSuffix);
        assert_eq!(suffix.unwrap().action, RuleAction::Direct);

        let kw = result.domain_rules.iter().find(|r| r.pattern == "dingding");
        assert!(kw.is_some());
        assert_eq!(kw.unwrap().kind, DomainRuleKind::DomainKeyword);

        let cn = result.domain_rules.iter().find(|r| r.pattern == "*.cn");
        assert!(cn.is_some());
        assert_eq!(cn.unwrap().kind, DomainRuleKind::DomainSuffix);

        let exact = result.domain_rules.iter().find(|r| r.pattern == "plaindomain.org");
        assert!(exact.is_some());
        assert_eq!(exact.unwrap().kind, DomainRuleKind::Domain);
        assert_eq!(exact.unwrap().action, RuleAction::Proxy);
    }

    #[test]
    fn parses_omega_user_file() {
        let path = std::path::Path::new("/home/jx/Downloads/ZeroOmegaOptions-2026-09-11T15_07_38.897Z.bak");
        if path.exists() {
            let content = std::fs::read_to_string(path).expect("read file");
            let result = parse_omega_rules(&content).expect("parse rules");
            assert_eq!(result.domain_rules.len(), 202);
            assert_eq!(result.ip_rules.len(), 15);
        }
    }
}

use crate::{DomainRuleKind, GlobalSettings, RuleAction};
use std::{collections::HashMap, net::IpAddr, path::PathBuf};

#[derive(Debug, Clone, Default)]
pub struct FlowContext {
    pub destination: Option<IpAddr>,
    pub domain: Option<String>,
    pub executable: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    CustomOverride,
    AppRule,
    DomainRule,
    IpRule,
    DefaultPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDecision {
    pub action: RuleAction,
    pub source: MatchSource,
}

/// 路由匹配引擎，支持对自定义覆盖、应用规则、域名规则、IP 规则及默认策略的分级匹配
#[derive(Debug, Clone)]
pub struct RoutingEngine {
    settings: GlobalSettings,
    exact_domain_map: HashMap<String, (usize, RuleAction)>,
    suffix_domain_map: HashMap<String, (usize, RuleAction)>,
    keyword_rules: Vec<(usize, String, RuleAction)>,
}

impl RoutingEngine {
    /// 创建路由引擎，并预编译域名规则索引以实现微秒级检索
    pub fn new(settings: GlobalSettings) -> Self {
        let mut exact_domain_map = HashMap::new();
        let mut suffix_domain_map = HashMap::new();
        let mut keyword_rules = Vec::new();

        let all_rules = settings
            .domain_rules
            .iter()
            .chain(settings.imported_domain_rules.iter());

        for (idx, rule) in all_rules.enumerate() {
            let pattern = rule.pattern.trim().trim_end_matches('.').to_ascii_lowercase();
            let clean = pattern.trim_start_matches('*').trim_start_matches('.').to_string();
            if clean.is_empty() {
                continue;
            }
            match rule.kind {
                DomainRuleKind::Domain => {
                    exact_domain_map.entry(clean).or_insert((idx, rule.action));
                }
                DomainRuleKind::DomainSuffix | DomainRuleKind::Legacy => {
                    suffix_domain_map.entry(clean).or_insert((idx, rule.action));
                }
                DomainRuleKind::DomainKeyword => {
                    keyword_rules.push((idx, clean, rule.action));
                }
            }
        }

        Self {
            settings,
            exact_domain_map,
            suffix_domain_map,
            keyword_rules,
        }
    }

    /// 对流量上下文执行路由决策
    pub fn decide(&self, flow: &FlowContext) -> RouteDecision {
        if let Some(destination) = flow.destination {
            if let Some(rule) = self
                .settings
                .custom_overrides
                .iter()
                .find(|rule| rule.network.contains(&destination))
            {
                return RouteDecision { action: rule.action, source: MatchSource::CustomOverride };
            }
        }

        if let Some(executable) = &flow.executable {
            if let Some(rule) = self.settings.app_rules.iter().find(|rule| &rule.executable == executable) {
                return RouteDecision { action: rule.action, source: MatchSource::AppRule };
            }
        }

        if let Some(domain) = &flow.domain {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if let Some(action) = self.match_domain(&domain) {
                return RouteDecision { action, source: MatchSource::DomainRule };
            }
        }

        if let Some(destination) = flow.destination {
            if let Some(rule) = self
                .settings
                .ip_rules
                .iter()
                .chain(self.settings.imported_ip_rules.iter())
                .find(|rule| rule.network.contains(&destination))
            {
                return RouteDecision { action: rule.action, source: MatchSource::IpRule };
            }
        }

        RouteDecision { action: self.settings.default_policy, source: MatchSource::DefaultPolicy }
    }

    /// 高性能匹配域名：优先检查精确与后缀哈希表，再进行有限关键字匹配，保留原有规则先后优先级
    fn match_domain(&self, domain: &str) -> Option<RuleAction> {
        let mut best_match: Option<(usize, RuleAction)> = None;

        // 1. 精确匹配
        if let Some(&(idx, action)) = self.exact_domain_map.get(domain) {
            best_match = Some((idx, action));
        }

        // 2. 后缀匹配（包含全域名与各级子域名）
        if let Some(&(idx, action)) = self.suffix_domain_map.get(domain) {
            if best_match.as_ref().map_or(true, |(best_idx, _)| idx < *best_idx) {
                best_match = Some((idx, action));
            }
        }

        let mut dot_idx = domain.find('.');
        while let Some(pos) = dot_idx {
            let suffix = &domain[pos + 1..];
            if let Some(&(idx, action)) = self.suffix_domain_map.get(suffix) {
                if best_match.as_ref().map_or(true, |(best_idx, _)| idx < *best_idx) {
                    best_match = Some((idx, action));
                }
            }
            dot_idx = domain[pos + 1..].find('.').map(|next| pos + 1 + next);
        }

        // 3. 关键字匹配
        let current_best_idx = best_match.as_ref().map(|(idx, _)| *idx).unwrap_or(usize::MAX);
        for &(idx, ref keyword, action) in &self.keyword_rules {
            if idx >= current_best_idx {
                break;
            }
            if domain.contains(keyword.as_str()) {
                best_match = Some((idx, action));
                break;
            }
        }

        best_match.map(|(_, action)| action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppRule, DomainRule};

    #[test]
    fn priority_is_override_app_domain_ip_default() {
        let mut settings = GlobalSettings::default();
        settings.custom_overrides.push(crate::config::IpRule {
            network: "10.0.0.0/8".parse().unwrap(),
            action: RuleAction::Block,
        });
        settings.app_rules.push(AppRule { executable: "/usr/bin/firefox".into(), action: RuleAction::Direct });
        settings.domain_rules.push(DomainRule {
            pattern: "*.example.com".into(),
            action: RuleAction::Proxy,
            kind: DomainRuleKind::Legacy,
        });
        settings.ip_rules.push(crate::config::IpRule {
            network: "10.0.0.0/8".parse().unwrap(),
            action: RuleAction::Direct,
        });

        let decision = RoutingEngine::new(settings).decide(&FlowContext {
            destination: Some("10.1.2.3".parse().unwrap()),
            domain: Some("api.example.com".into()),
            executable: Some("/usr/bin/firefox".into()),
        });
        assert_eq!(decision.action, RuleAction::Block);
        assert_eq!(decision.source, MatchSource::CustomOverride);
    }

    #[test]
    fn test_wildcard_domain_matching() {
        let mut settings = GlobalSettings::default();
        settings.default_policy = RuleAction::Direct;
        settings.domain_rules.push(DomainRule {
            pattern: "*.reddit.com".into(),
            action: RuleAction::Proxy,
            kind: DomainRuleKind::DomainSuffix,
        });

        let engine = RoutingEngine::new(settings);
        let d1 = engine.decide(&FlowContext {
            domain: Some("reddit.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d1.action, RuleAction::Proxy);
        assert_eq!(d1.source, MatchSource::DomainRule);

        let d2 = engine.decide(&FlowContext {
            domain: Some("www.reddit.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d2.action, RuleAction::Proxy);
        assert_eq!(d2.source, MatchSource::DomainRule);

        let d3 = engine.decide(&FlowContext {
            domain: Some("notreddit.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d3.action, RuleAction::Direct);
        assert_eq!(d3.source, MatchSource::DefaultPolicy);
    }

    #[test]
    fn test_exact_and_keyword_matching() {
        let mut settings = GlobalSettings::default();
        settings.default_policy = RuleAction::Direct;
        settings.domain_rules.push(DomainRule {
            pattern: "google.com".into(),
            action: RuleAction::Proxy,
            kind: DomainRuleKind::Domain,
        });
        settings.domain_rules.push(DomainRule {
            pattern: "openai".into(),
            action: RuleAction::Proxy,
            kind: DomainRuleKind::DomainKeyword,
        });

        let engine = RoutingEngine::new(settings);
        let d1 = engine.decide(&FlowContext {
            domain: Some("google.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d1.action, RuleAction::Proxy);
        assert_eq!(d1.source, MatchSource::DomainRule);

        let d2 = engine.decide(&FlowContext {
            domain: Some("sub.google.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d2.action, RuleAction::Direct); // exact domain does not match subdomains

        let d3 = engine.decide(&FlowContext {
            domain: Some("chat.openai.com".into()),
            ..FlowContext::default()
        });
        assert_eq!(d3.action, RuleAction::Proxy);
        assert_eq!(d3.source, MatchSource::DomainRule);
    }
}

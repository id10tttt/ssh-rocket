use crate::{DomainRule, DomainRuleKind, GlobalSettings, RuleAction};
use std::{net::IpAddr, path::PathBuf};

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

#[derive(Debug, Clone)]
pub struct RoutingEngine {
    settings: GlobalSettings,
}

impl RoutingEngine {
    pub fn new(settings: GlobalSettings) -> Self {
        Self { settings }
    }

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
            if let Some(rule) = self
                .settings
                .domain_rules
                .iter()
                .chain(self.settings.imported_domain_rules.iter())
                .find(|rule| domain_matches_rule(rule, &domain))
            {
                return RouteDecision { action: rule.action, source: MatchSource::DomainRule };
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
}

fn domain_matches_rule(rule: &DomainRule, domain: &str) -> bool {
    let pattern = rule.pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    let clean_pattern = pattern.trim_start_matches('*').trim_start_matches('.');
    match rule.kind {
        DomainRuleKind::Domain => domain == pattern || domain == clean_pattern,
        DomainRuleKind::DomainSuffix => {
            domain == clean_pattern || domain.ends_with(&format!(".{clean_pattern}"))
        }
        DomainRuleKind::DomainKeyword => domain.contains(&clean_pattern),
        DomainRuleKind::Legacy => {
            domain == clean_pattern || domain.ends_with(&format!(".{clean_pattern}"))
        }
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
}

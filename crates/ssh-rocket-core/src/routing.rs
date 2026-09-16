use crate::{GlobalSettings, RuleAction};
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
                .find(|rule| domain_matches(&rule.pattern, &domain))
            {
                return RouteDecision { action: rule.action, source: MatchSource::DomainRule };
            }
        }

        if let Some(destination) = flow.destination {
            if let Some(rule) = self.settings.ip_rules.iter().find(|rule| rule.network.contains(&destination)) {
                return RouteDecision { action: rule.action, source: MatchSource::IpRule };
            }
        }

        RouteDecision { action: self.settings.default_policy, source: MatchSource::DefaultPolicy }
    }
}

fn domain_matches(pattern: &str, domain: &str) -> bool {
    let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return domain == suffix || domain.ends_with(&format!(".{suffix}"));
    }
    domain == pattern
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
        settings.domain_rules.push(DomainRule { pattern: "*.example.com".into(), action: RuleAction::Proxy });
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
}

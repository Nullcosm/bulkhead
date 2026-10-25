use serde::{Deserialize, Serialize};

use crate::action::{Action, Verdict};
use crate::agent::AgentId;

/// Stable identifier for a policy rule (appears in denials and audit events).
pub type RuleId = String;

/// What a rule does when it matches an action.
///
/// Serialized lowercase (`"allow"` / `"deny"`) per the v1 policy file format
/// (docs/policy-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

/// A single policy rule.
///
/// v1 matching: the rule matches when `tools` (empty = any tool) and `agents`
/// (empty = any agent) both match. `condition` is reserved for structured
/// conditions and is **not evaluated yet** — that lands with the Cedar/Rego
/// adapter in milestone M3. See `docs/policy-model.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: RuleId,
    pub effect: Effect,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub agents: Vec<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<serde_json::Value>,
}

impl PolicyRule {
    /// True when this rule applies to `action`. Structured `condition` is not
    /// part of v1 matching (see struct docs).
    pub fn matches(&self, action: &Action) -> bool {
        let tool_ok = self.tools.is_empty() || self.tools.iter().any(|t| t == &action.tool);
        let agent_ok = self.agents.is_empty() || self.agents.contains(&action.agent);
        tool_ok && agent_ok
    }
}

/// An ordered collection of rules. First match wins; no match = deny.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PolicySet {
    pub rules: Vec<PolicyRule>,
}

impl PolicySet {
    pub fn new(rules: Vec<PolicyRule>) -> Self {
        Self { rules }
    }

    /// Evaluate `action` against the ordered rules. Deny-by-default: no match
    /// produces a denial, never an allow.
    pub fn evaluate(&self, action: &Action) -> Verdict {
        for rule in &self.rules {
            if rule.matches(action) {
                return match rule.effect {
                    Effect::Allow => Verdict::Allow,
                    Effect::Deny => Verdict::Deny {
                        reason: format!("denied by rule '{}'", rule.id),
                        rule_id: Some(rule.id.clone()),
                    },
                };
            }
        }
        Verdict::denied("no rule matched (deny by default)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, effect: Effect, tools: &[&str]) -> PolicyRule {
        PolicyRule {
            id: id.into(),
            effect,
            tools: tools.iter().map(|t| t.to_string()).collect(),
            agents: vec![],
            condition: None,
        }
    }

    #[test]
    fn deny_by_default() {
        let set = PolicySet::new(vec![rule("allow-reads", Effect::Allow, &["http.get"])]);
        let write = Action::new("agent-1", "db.query", "{}");
        assert!(matches!(set.evaluate(&write), Verdict::Deny { .. }));
    }

    #[test]
    fn first_match_wins() {
        let set = PolicySet::new(vec![
            rule("deny-writes", Effect::Deny, &["db.query"]),
            rule("allow-all", Effect::Allow, &[]),
        ]);
        let write = Action::new("agent-1", "db.query", "{}");
        match set.evaluate(&write) {
            Verdict::Deny {
                rule_id: Some(id), ..
            } => assert_eq!(id, "deny-writes"),
            other => panic!("expected denial by rule, got {other:?}"),
        }
        let read = Action::new("agent-1", "http.get", "{}");
        assert_eq!(set.evaluate(&read), Verdict::Allow);
    }

    #[test]
    fn agent_scope_filters() {
        let scoped = PolicyRule {
            id: "only-demo".into(),
            effect: Effect::Allow,
            tools: vec!["shell.run".into()],
            agents: vec![AgentId::new("demo-agent")],
            condition: None,
        };
        let set = PolicySet::new(vec![scoped]);
        let demo = Action::new("demo-agent", "shell.run", "{}");
        let other = Action::new("other-agent", "shell.run", "{}");
        assert_eq!(set.evaluate(&demo), Verdict::Allow);
        assert!(matches!(set.evaluate(&other), Verdict::Deny { .. }));
    }
}

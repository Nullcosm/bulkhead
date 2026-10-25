//! Policy engine: load the v1 JSON policy format and evaluate actions.
//!
//! See `docs/policy-model.md` for the file format. Evaluation semantics:
//! ordered rules, first match wins, **deny by default**. Rule matching is
//! tool patterns (`*`/`?` globs) × agent ids (exact) × structured conditions.
//!
//! Policies are validated at load time — duplicate rule ids, empty ids,
//! malformed conditions, or unknown operators make the load fail. A broken
//! policy file must never mean "no policy".

pub mod conditions;
pub mod glob;

use std::collections::HashSet;

use bulkhead_core::action::{Action, Verdict};
use bulkhead_core::error::{BulkheadError, Result};
use bulkhead_core::policy::{PolicyRule, PolicySet};

use conditions::Condition;
use glob::wildcard_match;

/// One loaded-and-validated rule: the serde model plus a parsed condition.
#[derive(Debug, Clone)]
struct ValidatedRule {
    rule: PolicyRule,
    condition: Option<Condition>,
}

/// The policy engine. Owns the active, validated rule set.
#[derive(Debug, Clone, Default)]
pub struct Engine {
    rules: Vec<ValidatedRule>,
}

impl Engine {
    pub fn new(set: PolicySet) -> Result<Self> {
        Self::validate(set)
    }

    /// Parse the v1 JSON policy file format and validate it.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let set: PolicySet = serde_json::from_slice(bytes)
            .map_err(|e| BulkheadError::Policy(format!("invalid policy JSON: {e}")))?;
        Self::validate(set)
    }

    fn validate(set: PolicySet) -> Result<Self> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut rules = Vec::with_capacity(set.rules.len());
        for rule in set.rules {
            if rule.id.trim().is_empty() {
                return Err(BulkheadError::Policy("rule with empty id".into()));
            }
            if !seen.insert(rule.id.clone()) {
                return Err(BulkheadError::Policy(format!(
                    "duplicate rule id '{}'",
                    rule.id
                )));
            }
            if rule.tools.iter().any(|t| t.trim().is_empty()) {
                return Err(BulkheadError::Policy(format!(
                    "rule '{}' has an empty tool pattern",
                    rule.id
                )));
            }
            if rule.agents.iter().any(|a| a.as_str().trim().is_empty()) {
                return Err(BulkheadError::Policy(format!(
                    "rule '{}' has an empty agent id",
                    rule.id
                )));
            }
            let condition = match &rule.condition {
                Some(value) => match Condition::parse(value) {
                    Ok(condition) => condition,
                    Err(message) => {
                        return Err(BulkheadError::Policy(format!(
                            "rule '{}': {message}",
                            rule.id
                        )));
                    }
                },
                None => None,
            };
            rules.push(ValidatedRule { rule, condition });
        }
        Ok(Self { rules })
    }

    /// Evaluate one action. First match wins; no match = deny.
    pub fn evaluate(&self, action: &Action) -> Verdict {
        for validated in &self.rules {
            if rule_matches(validated, action) {
                return match validated.rule.effect {
                    bulkhead_core::policy::Effect::Allow => Verdict::Allow,
                    bulkhead_core::policy::Effect::Deny => Verdict::Deny {
                        reason: format!("denied by rule '{}'", validated.rule.id),
                        rule_id: Some(validated.rule.id.clone()),
                    },
                };
            }
        }
        Verdict::denied("no rule matched (deny by default)")
    }

    /// The active rules, in evaluation order.
    pub fn rules(&self) -> impl Iterator<Item = &PolicyRule> {
        self.rules.iter().map(|v| &v.rule)
    }

    /// Number of active rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// Full v1 matching: tool globs × agent ids × condition.
fn rule_matches(validated: &ValidatedRule, action: &Action) -> bool {
    let rule = &validated.rule;
    let tool_ok = rule.tools.is_empty()
        || rule
            .tools
            .iter()
            .any(|pattern| wildcard_match(pattern, &action.tool));
    let agent_ok = rule.agents.is_empty() || rule.agents.contains(&action.agent);
    let condition_ok = match &validated.condition {
        Some(condition) => condition.matches(action),
        None => true,
    };
    tool_ok && agent_ok && condition_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(json: &str) -> Engine {
        Engine::from_json(json.as_bytes()).unwrap()
    }

    #[test]
    fn deny_by_default() {
        let engine = load(r#"{"rules":[]}"#);
        let action = Action::new("agent-1", "http.get", "{}");
        assert!(matches!(engine.evaluate(&action), Verdict::Deny { .. }));
    }

    #[test]
    fn first_match_wins() {
        let engine = load(
            r#"{"rules":[
                {"id":"deny-writes","effect":"deny","tools":["db.*"]},
                {"id":"allow-all","effect":"allow"}
            ]}"#,
        );
        let write = Action::new("agent-1", "db.query", "{}");
        match engine.evaluate(&write) {
            Verdict::Deny {
                rule_id: Some(id), ..
            } => assert_eq!(id, "deny-writes"),
            other => panic!("expected denial by rule, got {other:?}"),
        }
        assert_eq!(
            engine.evaluate(&Action::new("agent-1", "http.get", "{}")),
            Verdict::Allow
        );
    }

    #[test]
    fn tool_globs_match() {
        let engine = load(
            r#"{"rules":[
                {"id":"allow-http","effect":"allow","tools":["http.*"]}
            ]}"#,
        );
        assert_eq!(
            engine.evaluate(&Action::new("agent-1", "http.get", "{}")),
            Verdict::Allow
        );
        assert_eq!(
            engine.evaluate(&Action::new("agent-1", "http.post", "{}")),
            Verdict::Allow
        );
        assert!(matches!(
            engine.evaluate(&Action::new("agent-1", "db.query", "{}")),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn condition_narrows_a_rule() {
        let engine = load(
            r#"{"rules":[
                {"id":"deny-prod","effect":"deny","tools":["db.*"],
                 "condition":{"field":"resource","op":"prefix","value":"prod"}},
                {"id":"allow-staging","effect":"allow","tools":["db.*"],
                 "condition":{"field":"resource","op":"prefix","value":"staging"}}
            ]}"#,
        );
        let prod = Action::new("agent-1", "db.query", "{}").with_resource("prod.db");
        let staging = Action::new("agent-1", "db.query", "{}").with_resource("staging.db");
        let other = Action::new("agent-1", "db.query", "{}").with_resource("dev.db");

        assert!(matches!(
            engine.evaluate(&prod),
            Verdict::Deny {
                rule_id: Some(id),
                ..
            } if id == "deny-prod"
        ));
        assert_eq!(engine.evaluate(&staging), Verdict::Allow);
        // dev.db matches neither condition -> deny by default.
        assert!(matches!(engine.evaluate(&other), Verdict::Deny { .. }));
    }

    #[test]
    fn params_condition() {
        let engine = load(
            r#"{"rules":[
                {"id":"block-deletes","effect":"deny","tools":["db.*"],
                 "condition":{"field":"params.sql","op":"contains","value":"DELETE"}}
            ]}"#,
        );
        let delete = Action::new("agent-1", "db.query", r#"{"sql":"DELETE FROM users"}"#);
        assert!(matches!(engine.evaluate(&delete), Verdict::Deny { .. }));
    }

    #[test]
    fn duplicate_ids_rejected() {
        let err = Engine::from_json(
            br#"{"rules":[
                {"id":"a","effect":"allow"},
                {"id":"a","effect":"deny"}
            ]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate rule id 'a'"));
    }

    #[test]
    fn empty_id_rejected() {
        assert!(Engine::from_json(br#"{"rules":[{"id":"","effect":"allow"}]}"#).is_err());
    }

    #[test]
    fn bad_condition_rejected() {
        let err = Engine::from_json(
            br#"{"rules":[
                {"id":"a","effect":"allow","condition":{"field":"nope","op":"eq","value":"x"}}
            ]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("rule 'a'"));
    }

    #[test]
    fn loads_v1_json_with_version_field() {
        let engine = load(
            r#"{"version":1,"rules":[{"id":"allow-reads","effect":"allow","tools":["http.get"]}]}"#,
        );
        assert_eq!(engine.rule_count(), 1);
        assert_eq!(
            engine.evaluate(&Action::new("agent-1", "http.get", "{}")),
            Verdict::Allow
        );
    }

    #[test]
    fn malformed_json_is_an_error() {
        let err = Engine::from_json(b"{ not json").unwrap_err();
        assert!(matches!(err, BulkheadError::Policy(_)));
    }
}

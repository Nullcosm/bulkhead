//! Structured conditions for policy rules (milestone M3).
//!
//! A condition narrows a rule to actions whose extracted field value matches
//! an operator + operand. Conditions are validated at policy load time — an
//! unknown op or field fails the load, never silently disables a rule.

use bulkhead_core::action::Action;
use serde_json::Value;

/// The operator set of the v1 condition format.
pub const KNOWN_OPS: [&str; 7] = ["eq", "ne", "prefix", "suffix", "contains", "in", "exists"];

/// Roots extractable from an [`Action`]; any `params.`-prefixed dotted path
/// is also valid (e.g. `params.sql`).
pub fn valid_field(field: &str) -> bool {
    matches!(field, "agent" | "tool" | "resource")
        || (field.starts_with("params") && field.len() > "params".len())
}

/// One validated condition. `None` is used for "no condition" rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    field: String,
    op: String,
    value: Option<Value>,
}

impl Condition {
    /// Parse and validate a condition from the policy JSON.
    ///
    /// Returns `Ok(None)` for `null`/missing. Returns `Err(message)` for any
    /// malformed structure — loaders must fail the policy on error.
    pub fn parse(v: &Value) -> Result<Option<Self>, String> {
        if v.is_null() {
            return Ok(None);
        }
        let obj = v
            .as_object()
            .ok_or_else(|| "condition must be an object or null".to_string())?;

        let field = obj
            .get("field")
            .and_then(Value::as_str)
            .ok_or_else(|| "condition.field must be a string".to_string())?;
        if !valid_field(field) {
            return Err(format!(
                "condition.field '{field}' is not extractable (use agent, tool, resource, or params.<path>)"
            ));
        }

        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "condition.op must be a string".to_string())?;
        if !KNOWN_OPS.contains(&op) {
            return Err(format!(
                "condition.op '{op}' is unknown (known: {})",
                KNOWN_OPS.join(", ")
            ));
        }

        let value = obj.get("value").cloned();
        if op != "exists" && value.is_none() {
            return Err(format!("condition.op '{op}' requires a 'value'"));
        }

        Ok(Some(Self {
            field: field.to_string(),
            op: op.to_string(),
            value,
        }))
    }

    /// Evaluate the condition against one action.
    pub fn matches(&self, action: &Action) -> bool {
        match self.op.as_str() {
            "exists" => {
                let present = extract(action, &self.field).is_some();
                match &self.value {
                    Some(Value::Bool(want)) => present == *want,
                    _ => present,
                }
            }
            "in" => {
                let lhs = match extract(action, &self.field) {
                    Some(v) => v,
                    None => return false,
                };
                match &self.value {
                    Some(Value::Array(items)) => items.iter().any(|item| item == &lhs),
                    _ => false,
                }
            }
            op => {
                let lhs = match extract(action, &self.field) {
                    Some(v) => v,
                    None => return false,
                };
                let rhs = match &self.value {
                    Some(v) => v.clone(),
                    None => return false,
                };
                compare(op, &lhs, &rhs)
            }
        }
    }
}

/// String/equality comparison for the scalar ops.
fn compare(op: &str, lhs: &Value, rhs: &Value) -> bool {
    match op {
        "eq" | "ne" => {
            let equal = if let (Some(a), Some(b)) = (lhs.as_f64(), rhs.as_f64()) {
                a == b
            } else {
                lhs.as_str() == rhs.as_str()
            };
            if op == "eq" { equal } else { !equal }
        }
        "prefix" => match (lhs.as_str(), rhs.as_str()) {
            (Some(a), Some(b)) => a.starts_with(b),
            _ => false,
        },
        "suffix" => match (lhs.as_str(), rhs.as_str()) {
            (Some(a), Some(b)) => a.ends_with(b),
            _ => false,
        },
        "contains" => match (lhs.as_str(), rhs.as_str()) {
            (Some(a), Some(b)) => a.contains(b),
            _ => false,
        },
        _ => false,
    }
}

/// Extract the condition field from an action.
fn extract(action: &Action, field: &str) -> Option<Value> {
    match field {
        "agent" => Some(Value::String(action.agent.as_str().to_string())),
        "tool" => Some(Value::String(action.tool.clone())),
        "resource" => action.resource.clone().map(Value::String),
        _ => {
            let path = field.strip_prefix("params")?;
            let params: Value = serde_json::from_str(&action.params).ok()?;
            lookup(&params, path)
        }
    }
}

/// Dotted-path lookup into a JSON value ("" = the value itself).
fn lookup(v: &Value, path: &str) -> Option<Value> {
    let mut cur = v;
    for segment in path.trim_start_matches('.').split('.') {
        if segment.is_empty() {
            continue;
        }
        cur = cur.get(segment)?;
    }
    Some(cur.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bulkhead_core::agent::AgentId;

    fn action(tool: &str, params: &str, resource: Option<&str>) -> Action {
        let mut a = Action::new(AgentId::new("agent-1"), tool, params);
        if let Some(r) = resource {
            a = a.with_resource(r);
        }
        a
    }

    fn cond(json: &str) -> Condition {
        Condition::parse(&serde_json::from_str::<Value>(json).unwrap())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn prefix_on_resource() {
        let c = cond(r#"{"field":"resource","op":"prefix","value":"prod"}"#);
        assert!(c.matches(&action("db.query", "{}", Some("prod.db"))));
        assert!(!c.matches(&action("db.query", "{}", Some("staging.db"))));
        assert!(!c.matches(&action("db.query", "{}", None)));
    }

    #[test]
    fn params_dotted_lookup() {
        let c = cond(r#"{"field":"params.sql","op":"contains","value":"DELETE"}"#);
        assert!(c.matches(&action("db.query", r#"{"sql":"DELETE FROM users"}"#, None)));
        assert!(!c.matches(&action("db.query", r#"{"sql":"SELECT 1"}"#, None)));
    }

    #[test]
    fn in_operator() {
        let c = cond(r#"{"field":"tool","op":"in","value":["shell.run","file.write"]}"#);
        assert!(c.matches(&action("shell.run", "{}", None)));
        assert!(!c.matches(&action("http.get", "{}", None)));
    }

    #[test]
    fn exists_operator() {
        let c = cond(r#"{"field":"resource","op":"exists","value":true}"#);
        assert!(c.matches(&action("http.get", "{}", Some("https://x"))));
        assert!(!c.matches(&action("http.get", "{}", None)));
    }

    #[test]
    fn numeric_eq() {
        let c = cond(r#"{"field":"params.count","op":"eq","value":5}"#);
        assert!(c.matches(&action("tool.x", r#"{"count":5}"#, None)));
        assert!(!c.matches(&action("tool.x", r#"{"count":6}"#, None)));
    }

    #[test]
    fn validation_rejects_unknown_things() {
        assert!(
            Condition::parse(&serde_json::json!(null))
                .unwrap()
                .is_none()
        );
        assert!(
            Condition::parse(&serde_json::json!({"field":"nope","op":"eq","value":"x"})).is_err()
        );
        assert!(
            Condition::parse(&serde_json::json!({"field":"tool","op":"frobnicate","value":"x"}))
                .is_err()
        );
        assert!(Condition::parse(&serde_json::json!({"field":"tool","op":"eq"})).is_err());
    }
}

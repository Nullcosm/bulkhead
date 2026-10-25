use serde::{Deserialize, Serialize};

use crate::agent::AgentId;

/// One tool call or egress attempt made by an agent that Bulkhead must adjudicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// The agent that performed (or requested) the action.
    pub agent: AgentId,
    /// Tool name, e.g. `http.get`, `db.query`, `file.write`.
    pub tool: String,
    /// Tool parameters as a JSON string (opaque to the core model).
    pub params: String,
    /// Target resource if known: URL, table, path, bucket, ...
    pub resource: Option<String>,
}

impl Action {
    pub fn new(
        agent: impl Into<AgentId>,
        tool: impl Into<String>,
        params: impl Into<String>,
    ) -> Self {
        Self {
            agent: agent.into(),
            tool: tool.into(),
            params: params.into(),
            resource: None,
        }
    }

    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }
}

/// The adjudication of one [`Action`].
///
/// Bulkhead is deny-by-default: a denial is the ordinary outcome when no rule
/// matches — it is not an error condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Allow,
    Deny {
        reason: String,
        rule_id: Option<String>,
    },
}

impl Verdict {
    pub fn denied(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
            rule_id: None,
        }
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_with_resource() {
        let action = Action::new("agent-1", "http.get", "{\"url\":\"https://example.com\"}")
            .with_resource("https://example.com");
        assert_eq!(action.resource.as_deref(), Some("https://example.com"));
    }
}

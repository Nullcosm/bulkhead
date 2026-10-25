use serde::{Deserialize, Serialize};

/// A stable identifier for one agent (or one agent fleet).
///
/// For humans, `agent-<name>`. The identity crate binds these to short-lived
/// cryptographic credentials (structural in v0, signed from milestone M2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AgentId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl From<String> for AgentId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_json() {
        let id = AgentId::new("agent-1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"agent-1\"");
        assert_eq!(serde_json::from_str::<AgentId>(&json).unwrap(), id);
    }
}

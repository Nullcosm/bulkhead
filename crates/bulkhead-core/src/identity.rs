use serde::{Deserialize, Serialize};

use crate::agent::AgentId;

/// A short-lived credential issued to one agent.
///
/// The token is opaque to the core model. The `bulkhead-identity` crate issues
/// and verifies it; the v0 token is explicitly marked unsigned until ed25519
/// signing lands (milestone M2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// The agent this credential authorizes.
    pub agent: AgentId,
    /// Issuer name, e.g. `bulkhead.example.com`.
    pub issuer: String,
    /// Unix seconds at which the credential expires.
    pub expires_at_unix: u64,
    /// Opaque token presented with every request.
    pub token: String,
}

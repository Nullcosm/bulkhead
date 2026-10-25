use serde::{Deserialize, Serialize};

use crate::action::{Action, Verdict};

/// One immutable record of an evaluated action.
///
/// The audit trail is append-only: events are never updated or deleted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Monotonic event id within one log.
    pub id: u64,
    /// Unix timestamp in milliseconds at adjudication time.
    pub at_unix_ms: u64,
    /// The action that was adjudicated.
    pub action: Action,
    /// The verdict returned.
    pub verdict: Verdict,
    /// Budget state after the action (None = ledger not enabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_after_cents: Option<u64>,
}

impl AuditEvent {
    pub fn new(id: u64, at_unix_ms: u64, action: Action, verdict: Verdict) -> Self {
        Self {
            id,
            at_unix_ms,
            action,
            verdict,
            budget_after_cents: None,
        }
    }

    pub fn with_budget_after(mut self, remaining_cents: u64) -> Self {
        self.budget_after_cents = Some(remaining_cents);
        self
    }
}

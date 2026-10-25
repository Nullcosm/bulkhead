//! Bulkhead core — shared domain types for the Bulkhead agent-governance plane.
//!
//! This crate is deliberately dependency-light: it defines the data model
//! that every other crate speaks. Nothing here performs I/O or evaluation
//! beyond the trivial first-match semantics on [`policy::PolicySet`].

pub mod action;
pub mod agent;
pub mod audit;
pub mod error;
pub mod identity;
pub mod policy;

pub use action::{Action, Verdict};
pub use agent::AgentId;
pub use audit::AuditEvent;
pub use error::{BulkheadError, Result};
pub use identity::AgentIdentity;
pub use policy::{Effect, PolicyRule, PolicySet};

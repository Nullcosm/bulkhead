use std::fmt;

/// Crate-level error type. Kept small on purpose: Bulkhead crates fail loudly and
/// early — a governance plane must never silently continue past a broken
/// policy or audit sink.
#[derive(Debug)]
pub enum BulkheadError {
    Policy(String),
    Identity(String),
    Budget(String),
    Audit(String),
    Provider(String),
    Config(String),
}

pub type Result<T> = std::result::Result<T, BulkheadError>;

impl fmt::Display for BulkheadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(m) => write!(f, "policy: {m}"),
            Self::Identity(m) => write!(f, "identity: {m}"),
            Self::Budget(m) => write!(f, "budget: {m}"),
            Self::Audit(m) => write!(f, "audit: {m}"),
            Self::Provider(m) => write!(f, "provider: {m}"),
            Self::Config(m) => write!(f, "config: {m}"),
        }
    }
}

impl std::error::Error for BulkheadError {}

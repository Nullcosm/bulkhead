//! Per-agent budget metering and hard enforcement ("the kill switch").
//!
//! The ledger optionally persists budgets to a JSON file with atomic
//! replace-on-write (temp file + rename), so budgets survive gateway
//! restarts. A corrupt ledger file is a hard load error — Bulkhead fails
//! closed, never with an un-metered agent.
//!
//! Batching note: v1 persists on every mutation (simple + durable). For very
//! hot paths, persist-on-interval is a documented future optimization; the
//! `Ledger` API is unchanged by it.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use bulkhead_core::agent::AgentId;
use bulkhead_core::error::{BulkheadError, Result};

/// One agent's budget. All amounts are integer cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub limit_cents: u64,
    pub spent_cents: u64,
}

impl Budget {
    pub fn new(limit_cents: u64) -> Self {
        Self {
            limit_cents,
            spent_cents: 0,
        }
    }

    pub fn remaining_cents(&self) -> u64 {
        self.limit_cents.saturating_sub(self.spent_cents)
    }

    pub fn is_exhausted(&self) -> bool {
        self.spent_cents >= self.limit_cents
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LedgerFile {
    budgets: HashMap<AgentId, Budget>,
}

/// Budget ledger by agent, optionally backed by a JSON file.
#[derive(Debug, Default)]
pub struct Ledger {
    budgets: HashMap<AgentId, Budget>,
    file: Option<PathBuf>,
}

impl Ledger {
    /// In-memory ledger (tests, dev).
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a file-backed ledger, loading existing budgets. A missing file
    /// starts empty; a corrupt one is an error.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let budgets = if path.exists() {
            let bytes = std::fs::read(&path)
                .map_err(|e| BulkheadError::Budget(format!("read {}: {e}", path.display())))?;
            let file: LedgerFile = serde_json::from_slice(&bytes)
                .map_err(|e| BulkheadError::Budget(format!("parse {}: {e}", path.display())))?;
            file.budgets
        } else {
            HashMap::new()
        };
        Ok(Self {
            budgets,
            file: Some(path),
        })
    }

    pub fn set_budget(&mut self, agent: AgentId, budget: Budget) -> Result<()> {
        self.budgets.insert(agent, budget);
        self.persist()
    }

    pub fn budget(&self, agent: &AgentId) -> Option<&Budget> {
        self.budgets.get(agent)
    }

    /// Record spend of `cents` for `agent`.
    ///
    /// Returns `true` and updates the ledger when the charge fits the budget;
    /// returns `false` and changes nothing when it would overrun (or when the
    /// agent has no budget). Callers must treat `false` as "kill the action".
    pub fn charge(&mut self, agent: &AgentId, cents: u64) -> Result<bool> {
        let charged = match self.budgets.get_mut(agent) {
            Some(budget) => {
                if budget.spent_cents.saturating_add(cents) > budget.limit_cents {
                    false
                } else {
                    budget.spent_cents += cents;
                    true
                }
            }
            None => false,
        };
        if charged {
            self.persist()?;
        }
        Ok(charged)
    }

    /// Snapshot of every budget (for dashboards).
    pub fn snapshot(&self) -> HashMap<AgentId, Budget> {
        self.budgets.clone()
    }

    /// Atomic persist: write to `<path>.tmp`, fsync, rename over `<path>`.
    /// No-op for in-memory ledgers.
    fn persist(&self) -> Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        let tmp = path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(&LedgerFile {
            budgets: self.budgets.clone(),
        })
        .map_err(|e| BulkheadError::Budget(format!("serialize: {e}")))?;

        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)
            .map_err(|e| BulkheadError::Budget(format!("open {}: {e}", tmp.display())))?;
        file.write_all(&payload)
            .map_err(|e| BulkheadError::Budget(format!("write {}: {e}", tmp.display())))?;
        file.sync_all()
            .map_err(|e| BulkheadError::Budget(format!("sync {}: {e}", tmp.display())))?;
        drop(file);
        std::fs::rename(&tmp, path)
            .map_err(|e| BulkheadError::Budget(format!("rename to {}: {e}", path.display())))?;
        Ok(())
    }

    /// Explicitly persist (useful for tests; mutations already persist).
    pub fn persist_now(&self) -> Result<()> {
        self.persist()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn agent(name: &str) -> AgentId {
        AgentId::new(name)
    }

    fn temp_path(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "bulkhead-ledger-{}-{name}-{n}.json",
            std::process::id()
        ))
    }

    #[test]
    fn charge_within_budget() {
        let mut ledger = Ledger::new();
        ledger.set_budget(agent("a"), Budget::new(100)).unwrap();
        assert!(ledger.charge(&agent("a"), 60).unwrap());
        assert_eq!(ledger.budget(&agent("a")).unwrap().remaining_cents(), 40);
    }

    #[test]
    fn overrun_is_rejected_without_mutation() {
        let mut ledger = Ledger::new();
        ledger.set_budget(agent("a"), Budget::new(100)).unwrap();
        assert!(ledger.charge(&agent("a"), 80).unwrap());
        assert!(!ledger.charge(&agent("a"), 30).unwrap());
        assert_eq!(ledger.budget(&agent("a")).unwrap().spent_cents, 80);
    }

    #[test]
    fn unknown_agent_never_charged() {
        let mut ledger = Ledger::new();
        assert!(!ledger.charge(&agent("ghost"), 1).unwrap());
    }

    #[test]
    fn exhausted_budget_is_exhausted() {
        let mut ledger = Ledger::new();
        ledger.set_budget(agent("a"), Budget::new(50)).unwrap();
        ledger.charge(&agent("a"), 50).unwrap();
        assert!(ledger.budget(&agent("a")).unwrap().is_exhausted());
    }

    #[test]
    fn file_ledger_survives_reopen() {
        let path = temp_path("roundtrip");
        {
            let mut ledger = Ledger::open(&path).unwrap();
            ledger.set_budget(agent("a"), Budget::new(100)).unwrap();
            ledger.charge(&agent("a"), 40).unwrap();
        }
        let ledger = Ledger::open(&path).unwrap();
        let budget = ledger.budget(&agent("a")).unwrap();
        assert_eq!(budget.limit_cents, 100);
        assert_eq!(budget.spent_cents, 40);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_ledger_file_is_an_error() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let err = Ledger::open(&path).unwrap_err();
        assert!(matches!(err, BulkheadError::Budget(_)));
        let _ = std::fs::remove_file(&path);
    }
}

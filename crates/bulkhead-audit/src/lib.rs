//! Append-only audit trail.
//!
//! Two sinks in v0: in-memory (tests, dev) and JSONL file (production-shaped).
//! The JSONL format is one [`AuditEvent`] per line — the same format the
//! future ClickHouse/OpenSearch ingest will consume.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use bulkhead_core::agent::AgentId;
use bulkhead_core::audit::AuditEvent;
use bulkhead_core::error::{BulkheadError, Result};

/// Append-only log of [`AuditEvent`]s.
pub struct AuditLog {
    sink: Sink,
    next_id: u64,
}

enum Sink {
    Memory(Vec<AuditEvent>),
    File {
        path: PathBuf,
        writer: BufWriter<File>,
    },
}

impl AuditLog {
    pub fn memory() -> Self {
        Self {
            sink: Sink::Memory(Vec::new()),
            next_id: 0,
        }
    }

    /// Open (or create) a JSONL audit file in append mode.
    ///
    /// Event ids continue from the existing line count, so ids stay monotonic
    /// across gateway restarts.
    pub fn file(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())
            .map_err(|e| {
                BulkheadError::Audit(format!("cannot open {}: {e}", path.as_ref().display()))
            })?;
        let next_id = match std::fs::read_to_string(path.as_ref()) {
            Ok(text) => text.lines().filter(|l| !l.trim().is_empty()).count() as u64,
            Err(_) => 0,
        };
        Ok(Self {
            sink: Sink::File {
                path: path.as_ref().to_path_buf(),
                writer: BufWriter::new(file),
            },
            next_id,
        })
    }

    /// Append one event (its `id` is assigned by the log). Returns the id.
    pub fn append(&mut self, mut event: AuditEvent) -> Result<u64> {
        event.id = self.next_id;
        self.next_id += 1;
        let id = event.id;
        match &mut self.sink {
            Sink::Memory(events) => events.push(event),
            Sink::File { writer, .. } => {
                let line = serde_json::to_string(&event)
                    .map_err(|e| BulkheadError::Audit(format!("serialize: {e}")))?;
                writer
                    .write_all(line.as_bytes())
                    .map_err(|e| BulkheadError::Audit(format!("write: {e}")))?;
                writer
                    .write_all(b"\n")
                    .map_err(|e| BulkheadError::Audit(format!("write: {e}")))?;
                // Flush per event: auditors must be able to read the trail at
                // any moment, not at close time.
                writer
                    .flush()
                    .map_err(|e| BulkheadError::Audit(format!("flush: {e}")))?;
            }
        }
        Ok(id)
    }

    /// Serialize every event (all agents) as JSONL text — one event per line.
    pub fn dump_jsonl(&self) -> Result<String> {
        match &self.sink {
            Sink::Memory(events) => {
                let lines = events
                    .iter()
                    .map(|e| {
                        serde_json::to_string(e)
                            .map_err(|e| BulkheadError::Audit(format!("serialize: {e}")))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(lines.join("\n"))
            }
            Sink::File { path, .. } => std::fs::read_to_string(path)
                .map_err(|e| BulkheadError::Audit(format!("read {}: {e}", path.display()))),
        }
    }

    /// Every event for `agent`, oldest first.
    pub fn query(&self, agent: &AgentId) -> Result<Vec<AuditEvent>> {
        let all = match &self.sink {
            Sink::Memory(events) => events.clone(),
            Sink::File { path, .. } => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| BulkheadError::Audit(format!("read {}: {e}", path.display())))?;
                text.lines()
                    .map(|line| {
                        serde_json::from_str::<AuditEvent>(line)
                            .map_err(|e| BulkheadError::Audit(format!("parse audit line: {e}")))
                    })
                    .collect::<Result<Vec<_>>>()?
            }
        };
        Ok(all
            .into_iter()
            .filter(|e| &e.action.agent == agent)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bulkhead_core::action::{Action, Verdict};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique per-process suffix for test fixtures.
    fn test_suffix() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        (std::process::id() as u64) << 32 | n
    }

    fn event(agent: &str, tool: &str) -> AuditEvent {
        AuditEvent::new(
            0,
            1_700_000_000_000,
            Action::new(AgentId::new(agent), tool, "{}"),
            Verdict::Allow,
        )
    }

    #[test]
    fn memory_sink_appends_and_queries() {
        let mut log = AuditLog::memory();
        assert_eq!(log.append(event("a", "http.get")).unwrap(), 0);
        assert_eq!(log.append(event("b", "http.get")).unwrap(), 1);
        assert_eq!(log.append(event("a", "db.query")).unwrap(), 2);

        let a = log.query(&AgentId::new("a")).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].id, 0);
        assert_eq!(a[1].id, 2);
    }

    #[test]
    fn file_sink_roundtrips() {
        let path =
            std::env::temp_dir().join(format!("bulkhead-audit-test-{}.jsonl", test_suffix()));
        let mut log = AuditLog::file(&path).unwrap();
        log.append(event("a", "http.get")).unwrap();
        log.append(event("a", "db.query")).unwrap();

        // The trail must be readable while the writer is still open.
        let a = log.query(&AgentId::new("a")).unwrap();
        assert_eq!(a.len(), 2);
        assert!(a[0].verdict.is_allowed());

        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn denied_events_are_recorded_too() {
        let mut log = AuditLog::memory();
        let mut e = event("a", "db.query");
        e.verdict = Verdict::Deny {
            reason: "denied by rule 'deny-prod-writes'".into(),
            rule_id: Some("deny-prod-writes".into()),
        };
        log.append(e).unwrap();
        assert_eq!(log.query(&AgentId::new("a")).unwrap().len(), 1);
    }

    #[test]
    fn ids_continue_across_reopen() {
        let path =
            std::env::temp_dir().join(format!("bulkhead-audit-cont-{}.jsonl", test_suffix()));
        {
            let mut log = AuditLog::file(&path).unwrap();
            assert_eq!(log.append(event("a", "http.get")).unwrap(), 0);
            assert_eq!(log.append(event("a", "db.query")).unwrap(), 1);
        }
        {
            let mut log = AuditLog::file(&path).unwrap();
            assert_eq!(log.append(event("a", "http.post")).unwrap(), 2);
        }
        let _ = std::fs::remove_file(&path);
    }
}

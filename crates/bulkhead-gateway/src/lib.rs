//! The Bulkhead gateway: one place where every agent action is adjudicated.
//!
//! [`Gateway::check`] is the adjudication loop — policy → budget → audit.
//! [`server::serve`] exposes it over the HTTP API v1 (docs/architecture.md).

pub mod server;

use std::path::PathBuf;

use bulkhead_audit::AuditLog;
use bulkhead_core::action::{Action, Verdict};
use bulkhead_core::agent::AgentId;
use bulkhead_core::audit::AuditEvent;
use bulkhead_core::error::{BulkheadError, Result};
use bulkhead_core::identity::AgentIdentity;
use bulkhead_core::policy::PolicyRule;
use bulkhead_identity::{Issuer, Verifier};
use bulkhead_ledger::{Budget, Ledger};
use bulkhead_policy::Engine;

/// Everything the gateway needs, supplied by the operator.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Address the HTTP API listens on.
    pub listen: String,
    /// Path to the v1 policy JSON file.
    pub policy_file: PathBuf,
    /// Default budget for agents without an explicit one (cents).
    pub default_budget_cents: u64,
    /// Where the JSONL audit trail is written.
    pub audit_file: PathBuf,
    /// Issuer name embedded in issued credentials.
    pub issuer_name: String,
    /// Shared secret for HMAC token signing/verification. Rotate by
    /// restarting the gateway with a new value; tokens are short-lived.
    pub identity_secret: String,
    /// Budget ledger file. Empty path = in-memory ledger (tests, dev).
    pub ledger_file: PathBuf,
}

/// What [`Gateway::check`] returns to the caller (and, from M2, the proxy).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub verdict: Verdict,
    /// Remaining budget after the charge; `None` when the verdict is a denial
    /// or the ledger has no budget for the agent (matches the API v1 spec).
    pub remaining_cents: Option<u64>,
}

/// The assembled control plane.
pub struct Gateway {
    engine: Engine,
    issuer: Issuer,
    verifier: Verifier,
    ledger: Ledger,
    audit: AuditLog,
    default_budget_cents: u64,
}

impl Gateway {
    /// Load the full control plane from `config`.
    ///
    /// A missing or malformed policy file, or an unwritable audit file, is a
    /// hard error: the gateway refuses to start ungoverned.
    pub fn load(config: &GatewayConfig) -> Result<Self> {
        let policy_bytes = std::fs::read(&config.policy_file).map_err(|e| {
            BulkheadError::Config(format!("read {}: {e}", config.policy_file.display()))
        })?;
        let engine = Engine::from_json(&policy_bytes)?;
        let issuer = Issuer::new(
            config.issuer_name.clone(),
            3600,
            config.identity_secret.as_bytes(),
        );
        let verifier = Verifier::new(
            [config.issuer_name.clone()],
            config.identity_secret.as_bytes(),
        );
        let audit = AuditLog::file(&config.audit_file)?;
        let ledger = if config.ledger_file.as_os_str().is_empty() {
            Ledger::new()
        } else {
            Ledger::open(&config.ledger_file)?
        };
        Ok(Self {
            engine,
            issuer,
            verifier,
            ledger,
            audit,
            default_budget_cents: config.default_budget_cents,
        })
    }

    /// Issue a short-lived credential for `agent`.
    pub fn issue_identity(&self, agent: AgentId) -> AgentIdentity {
        self.issuer.issue(agent)
    }

    /// Authenticate a bearer token (signature, issuer trust, expiry) and
    /// return the identity it carries.
    pub fn authenticate_token(&self, token: &str) -> Result<AgentIdentity> {
        self.verifier.authenticate(token)
    }

    /// The active policy rules, in evaluation order.
    pub fn rules(&self) -> impl Iterator<Item = &PolicyRule> {
        self.engine.rules()
    }

    /// Every audit event for `agent`, oldest first.
    pub fn audit_events(&self, agent: &AgentId) -> Result<Vec<AuditEvent>> {
        self.audit.query(agent)
    }

    /// Give `agent` an explicit budget (overrides the default).
    pub fn set_budget(&mut self, agent: AgentId, limit_cents: u64) -> Result<()> {
        self.ledger.set_budget(agent, Budget::new(limit_cents))
    }

    /// Number of active policy rules.
    pub fn rule_count(&self) -> usize {
        self.engine.rule_count()
    }

    /// Adjudicate one action: policy → (allow ⇒ budget charge) → audit.
    ///
    /// Denied and over-budget actions are always audited. This is the single
    /// choke point every provider and SDK funnels through.
    pub fn check(&mut self, action: &Action) -> Result<CheckOutcome> {
        let at_unix_ms = now_millis();

        // Every agent without an explicit budget gets the default.
        if self.ledger.budget(&action.agent).is_none() {
            self.ledger
                .set_budget(action.agent.clone(), Budget::new(self.default_budget_cents))?;
        }

        let verdict = match self.engine.evaluate(action) {
            Verdict::Allow => {
                // v0 meters a flat 1 cent per action; real costing lands with
                // provider metering (M6).
                if self.ledger.charge(&action.agent, 1)? {
                    Verdict::Allow
                } else {
                    Verdict::denied("budget exhausted (kill switch)")
                }
            }
            verdict => verdict,
        };

        let remaining_cents = if verdict.is_allowed() {
            self.ledger
                .budget(&action.agent)
                .map(|b| b.remaining_cents())
        } else {
            None
        };

        let mut event = AuditEvent::new(0, at_unix_ms, action.clone(), verdict.clone());
        if let Some(remaining) = remaining_cents {
            event = event.with_budget_after(remaining);
        }
        self.audit.append(event)?;

        Ok(CheckOutcome {
            verdict,
            remaining_cents,
        })
    }

    /// Run the three-action demo (allowed / policy-blocked / budget-killed)
    /// against the live control plane and print every adjudication plus the
    /// resulting audit trail. See docs/demo.md.
    pub fn demo(&mut self) -> Result<()> {
        println!("=== Bulkhead demo: three moments, one control plane ===\n");
        let agent = AgentId::new("demo-agent");
        // A deliberately small budget so the kill switch fires in the script.
        self.set_budget(agent.clone(), 3)?;

        let actions = [
            (
                Action::new(
                    agent.clone(),
                    "http.get",
                    "{\"url\":\"https://example.com\"}",
                )
                .with_resource("https://example.com"),
                "read a public URL",
            ),
            (
                Action::new(agent.clone(), "db.query", "{\"sql\":\"DELETE FROM users\"}")
                    .with_resource("prod.db"),
                "write to prod DB",
            ),
            (
                Action::new(
                    agent.clone(),
                    "db.query",
                    "{\"sql\":\"INSERT INTO staging.users VALUES (1)\"}",
                )
                .with_resource("staging.db"),
                "write to staging DB",
            ),
            (
                Action::new(
                    agent.clone(),
                    "http.get",
                    "{\"url\":\"https://api.example.com/a\"}",
                )
                .with_resource("https://api.example.com/a"),
                "more reads until budget dies",
            ),
            (
                Action::new(
                    agent.clone(),
                    "http.get",
                    "{\"url\":\"https://api.example.com/b\"}",
                )
                .with_resource("https://api.example.com/b"),
                "one read too many",
            ),
        ];

        for (i, (action, story)) in actions.iter().enumerate() {
            let outcome = self.check(action)?;
            let verdict = match &outcome.verdict {
                Verdict::Allow => "ALLOWED".to_string(),
                Verdict::Deny { reason, .. } => format!("DENIED: {reason}"),
            };
            println!(
                "  {:>2}. {:<10} {:<30} -> {}",
                i + 1,
                action.tool,
                story,
                verdict
            );
        }

        println!("\n=== audit trail ===");
        println!("{}", self.audit.dump_jsonl()?);
        Ok(())
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &std::path::Path, name: &str) -> GatewayConfig {
        let policy_file = dir.join(format!("{name}.policy.json"));
        std::fs::write(
            &policy_file,
            r#"{
                "version": 1,
                "rules": [
                    { "id": "deny-prod-writes", "effect": "deny", "tools": ["db.query"] },
                    { "id": "allow-reads", "effect": "allow", "tools": ["http.get"] }
                ]
            }"#,
        )
        .unwrap();
        GatewayConfig {
            listen: "127.0.0.1:7373".into(),
            policy_file,
            default_budget_cents: 10_000,
            audit_file: dir.join(format!("{name}.audit.jsonl")),
            issuer_name: "bulkhead.test".into(),
            identity_secret: "test-secret".into(),
            ledger_file: PathBuf::new(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bulkhead-gw-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn allowed_action_charges_budget_and_audits() {
        let dir = temp("allow");
        let mut gw = Gateway::load(&config(&dir, "allow")).unwrap();
        let action = Action::new("agent-1", "http.get", "{}");
        let outcome = gw.check(&action).unwrap();
        assert_eq!(outcome.verdict, Verdict::Allow);
        assert_eq!(outcome.remaining_cents, Some(9_999));
        let events = gw.audit.query(&AgentId::new("agent-1")).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].budget_after_cents, Some(9_999));
    }

    #[test]
    fn denied_action_is_audited_but_not_charged() {
        let dir = temp("deny");
        let mut gw = Gateway::load(&config(&dir, "deny")).unwrap();
        let action = Action::new("agent-1", "db.query", "{}");
        let outcome = gw.check(&action).unwrap();
        match &outcome.verdict {
            Verdict::Deny {
                rule_id: Some(id), ..
            } => assert_eq!(id, "deny-prod-writes"),
            other => panic!("expected denial by rule, got {other:?}"),
        }
        assert_eq!(outcome.remaining_cents, None);
        // Budget untouched.
        assert_eq!(
            gw.ledger
                .budget(&AgentId::new("agent-1"))
                .unwrap()
                .remaining_cents(),
            10_000
        );
        // But the attempt is on the trail.
        assert_eq!(gw.audit.query(&AgentId::new("agent-1")).unwrap().len(), 1);
    }

    #[test]
    fn kill_switch_fires_on_budget_exhaustion() {
        let dir = temp("kill");
        let mut gw = Gateway::load(&config(&dir, "kill")).unwrap();
        let agent = AgentId::new("agent-1");
        gw.set_budget(agent.clone(), 1).unwrap();
        let read = Action::new(agent.clone(), "http.get", "{}");

        let first = gw.check(&read).unwrap();
        assert_eq!(first.verdict, Verdict::Allow);
        assert_eq!(first.remaining_cents, Some(0));

        let second = gw.check(&read).unwrap();
        match &second.verdict {
            Verdict::Deny { reason, rule_id } => {
                assert!(reason.contains("budget exhausted"));
                assert!(rule_id.is_none());
            }
            other => panic!("expected kill switch, got {other:?}"),
        }
        // The kill is audited too.
        assert_eq!(gw.audit.query(&agent).unwrap().len(), 2);
    }

    #[test]
    fn missing_policy_file_refuses_to_start() {
        let dir = temp("nopolicy");
        let cfg = GatewayConfig {
            policy_file: dir.join("does-not-exist.json"),
            audit_file: dir.join("nopolicy.audit.jsonl"),
            ..config(&dir, "nopolicy")
        };
        let err = Gateway::load(&cfg).err().expect("load must fail");
        assert!(matches!(err, BulkheadError::Config(_)));
    }

    #[test]
    fn budgets_survive_restart_with_ledger_file() {
        let dir = temp("restart");
        let mut cfg = config(&dir, "restart");
        cfg.ledger_file = dir.join("restart.ledger.json");
        let agent = AgentId::new("agent-1");
        {
            let mut gw = Gateway::load(&cfg).unwrap();
            gw.set_budget(agent.clone(), 10).unwrap();
            let outcome = gw
                .check(&Action::new(agent.clone(), "http.get", "{}"))
                .unwrap();
            assert_eq!(outcome.remaining_cents, Some(9));
        }
        let gw = Gateway::load(&cfg).unwrap();
        // Budget state (and default-budget bootstrap) survived the restart.
        assert_eq!(gw.ledger.budget(&agent).unwrap().spent_cents, 1);
        assert_eq!(gw.ledger.budget(&agent).unwrap().limit_cents, 10);
    }
}

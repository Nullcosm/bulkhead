//! HTTP API (v1) for the gateway — the contract is documented in
//! docs/architecture.md. The wire DTOs here are the stable API surface;
//! internal types may evolve independently.
//!
//! v1 handles requests sequentially. Concurrency arrives when the proxy
//! moves to a threaded/tokio core (post-M6); the adjudication loop is the
//! same either way.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Sender};

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Request, Response, StatusCode};

use bulkhead_core::action::{Action, Verdict};
use bulkhead_core::agent::AgentId;
use bulkhead_core::error::{BulkheadError, Result};

use crate::Gateway;

#[derive(Debug, Deserialize)]
struct CheckRequest {
    agent: String,
    token: String,
    tool: String,
    params: String,
    #[serde(default)]
    resource: Option<String>,
}

#[derive(Debug, Serialize)]
struct VerdictDto {
    allow: bool,
    reason: String,
    rule_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct CheckResponse {
    verdict: VerdictDto,
    remaining_cents: Option<u64>,
}

/// Serve the HTTP API on `listen` until `stop` is set.
///
/// `addr_report` (test helper): after binding, the actual bound address is
/// sent — use it with `listen = "127.0.0.1:0"` to get an ephemeral port.
pub fn serve(
    mut gateway: Gateway,
    listen: &str,
    stop: Arc<AtomicBool>,
    addr_report: Option<Sender<SocketAddr>>,
) -> Result<()> {
    let server = tiny_http::Server::http(listen)
        .map_err(|e| BulkheadError::Config(format!("cannot listen on {listen}: {e}")))?;
    if let (Some(tx), tiny_http::ListenAddr::IP(addr)) = (addr_report, server.server_addr()) {
        let _ = tx.send(addr);
    }
    while !stop.load(Ordering::Relaxed) {
        let request = match server.recv() {
            Ok(request) => request,
            // Recoverable accept error; keep serving.
            Err(_) => continue,
        };
        handle(&mut gateway, request);
    }
    Ok(())
}

fn handle(gateway: &mut Gateway, request: Request) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    match (request.method().as_str(), path.as_str()) {
        ("POST", "/v1/check") => handle_check(gateway, request),
        ("GET", "/healthz") => respond_json(
            request,
            StatusCode(200),
            serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }),
        ),
        ("GET", "/v1/policy") => {
            let rules: Vec<&bulkhead_core::policy::PolicyRule> = gateway.rules().collect();
            respond_json(
                request,
                StatusCode(200),
                serde_json::json!({ "rules": rules }),
            )
        }
        ("GET", "/v1/audit") => handle_audit(gateway, request, &url),
        _ => respond_json(
            request,
            StatusCode(404),
            serde_json::json!({ "error": "not found" }),
        ),
    }
}

fn handle_check(gateway: &mut Gateway, mut request: Request) {
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        return respond_json(
            request,
            StatusCode(400),
            serde_json::json!({ "error": "cannot read body" }),
        );
    }
    let req: CheckRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => {
            return respond_json(
                request,
                StatusCode(400),
                serde_json::json!({ "error": format!("malformed request: {e}") }),
            );
        }
    };

    let identity = match gateway.authenticate_token(&req.token) {
        Ok(identity) => identity,
        Err(e) => {
            return respond_json(
                request,
                StatusCode(401),
                serde_json::json!({ "error": e.to_string() }),
            );
        }
    };
    if identity.agent.as_str() != req.agent {
        return respond_json(
            request,
            StatusCode(401),
            serde_json::json!({ "error": "token does not match agent" }),
        );
    }

    let mut action = Action::new(identity.agent, req.tool, req.params);
    if let Some(resource) = req.resource {
        action = action.with_resource(resource);
    }

    match gateway.check(&action) {
        Ok(outcome) => {
            let verdict = match &outcome.verdict {
                Verdict::Allow => VerdictDto {
                    allow: true,
                    reason: String::new(),
                    rule_id: None,
                },
                Verdict::Deny { reason, rule_id } => VerdictDto {
                    allow: false,
                    reason: reason.clone(),
                    rule_id: rule_id.clone(),
                },
            };
            respond_json(
                request,
                StatusCode(200),
                serde_json::json!(CheckResponse {
                    verdict,
                    remaining_cents: outcome.remaining_cents,
                }),
            );
        }
        Err(e) => respond_json(
            request,
            StatusCode(500),
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

fn handle_audit(gateway: &Gateway, request: Request, url: &str) {
    let agent = url
        .split("agent=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .unwrap_or("");
    if agent.is_empty() {
        return respond_json(
            request,
            StatusCode(400),
            serde_json::json!({ "error": "missing ?agent=" }),
        );
    }
    match gateway.audit_events(&AgentId::new(agent)) {
        Ok(events) => respond_json(
            request,
            StatusCode(200),
            serde_json::json!({ "events": events }),
        ),
        Err(e) => respond_json(
            request,
            StatusCode(500),
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

fn respond_json(request: Request, status: StatusCode, body: serde_json::Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("static header"),
        );
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, mpsc};

    use crate::{Gateway, GatewayConfig};

    fn config(dir: &std::path::Path) -> GatewayConfig {
        let policy_file = dir.join("server.policy.json");
        std::fs::write(
            &policy_file,
            r#"{
                "version": 1,
                "rules": [
                    { "id": "deny-prod-writes", "effect": "deny", "tools": ["db.*"],
                      "condition": { "field": "resource", "op": "prefix", "value": "prod" } },
                    { "id": "allow-staging-writes", "effect": "allow", "tools": ["db.*"],
                      "condition": { "field": "resource", "op": "prefix", "value": "staging" } },
                    { "id": "allow-reads", "effect": "allow", "tools": ["http.*"] }
                ]
            }"#,
        )
        .unwrap();
        GatewayConfig {
            listen: "127.0.0.1:0".into(),
            policy_file,
            default_budget_cents: 3,
            audit_file: dir.join("server.audit.jsonl"),
            issuer_name: "bulkhead.test".into(),
            identity_secret: "test-secret".into(),
            ledger_file: PathBuf::new(),
        }
    }

    /// Start a real gateway HTTP server on an ephemeral port; returns its
    /// base URL and the stop flag.
    fn start_server(dir: &std::path::Path) -> (String, Arc<AtomicBool>) {
        let gateway = Gateway::load(&config(dir)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_stop = stop.clone();
        let listen = "127.0.0.1:0".to_string();
        std::thread::spawn(move || {
            let _ = serve(gateway, &listen, thread_stop, Some(tx));
        });
        let addr = rx.recv().unwrap();
        (format!("http://{addr}"), stop)
    }

    fn issue_token(dir: &std::path::Path) -> String {
        let gateway = Gateway::load(&config(dir)).unwrap();
        gateway
            .issue_identity(bulkhead_core::agent::AgentId::new("demo-agent"))
            .token
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulkhead-srv-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn full_http_lifecycle() {
        let dir = temp("lifecycle");
        let (base, stop) = start_server(&dir);
        let token = issue_token(&dir);

        // Health.
        let health: serde_json::Value = ureq::get(&format!("{base}/healthz"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(health["status"], "ok");

        // Policy is visible.
        let policy: serde_json::Value = ureq::get(&format!("{base}/v1/policy"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(policy["rules"].as_array().unwrap().len(), 3);

        // Allowed action.
        let allowed: serde_json::Value = ureq::post(&format!("{base}/v1/check"))
            .send_json(serde_json::json!({
                "agent": "demo-agent",
                "token": token,
                "tool": "http.get",
                "params": "{\"url\":\"https://example.com\"}",
                "resource": "https://example.com"
            }))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(allowed["verdict"]["allow"], true);
        assert_eq!(allowed["remaining_cents"], 2);

        // Prod write blocked by the condition-bearing rule.
        let blocked: serde_json::Value = ureq::post(&format!("{base}/v1/check"))
            .send_json(serde_json::json!({
                "agent": "demo-agent",
                "token": token,
                "tool": "db.query",
                "params": "{\"sql\":\"DELETE FROM users\"}",
                "resource": "prod.db"
            }))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(blocked["verdict"]["allow"], false);
        assert_eq!(blocked["verdict"]["rule_id"], "deny-prod-writes");

        // Staging write passes the condition.
        let staging: serde_json::Value = ureq::post(&format!("{base}/v1/check"))
            .send_json(serde_json::json!({
                "agent": "demo-agent",
                "token": token,
                "tool": "db.query",
                "params": "{\"sql\":\"INSERT INTO t VALUES (1)\"}",
                "resource": "staging.db"
            }))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(staging["verdict"]["allow"], true);
        assert_eq!(staging["remaining_cents"], 1);

        // Bad token => 401.
        let unauthorized = ureq::post(&format!("{base}/v1/check"))
            .send_json(serde_json::json!({
                "agent": "demo-agent",
                "token": "bulkhead.v1.forged.token",
                "tool": "http.get",
                "params": "{}"
            }))
            .unwrap_err();
        assert!(matches!(unauthorized, ureq::Error::Status(401, _)));

        // Token/agent mismatch => 401.
        let mismatched = ureq::post(&format!("{base}/v1/check"))
            .send_json(serde_json::json!({
                "agent": "other-agent",
                "token": token,
                "tool": "http.get",
                "params": "{}"
            }))
            .unwrap_err();
        assert!(matches!(mismatched, ureq::Error::Status(401, _)));

        // Audit trail visible over HTTP.
        let audit: serde_json::Value = ureq::get(&format!("{base}/v1/audit?agent=demo-agent"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(audit["events"].as_array().unwrap().len(), 3);

        // Malformed body => 400.
        let bad = ureq::post(&format!("{base}/v1/check"))
            .send_string("{ not json")
            .unwrap_err();
        assert!(matches!(bad, ureq::Error::Status(400, _)));

        // Shut down: signal stop, then one last request unblocks recv().
        stop.store(true, Ordering::Relaxed);
        let _ = ureq::get(&format!("{base}/healthz")).call();
    }
}

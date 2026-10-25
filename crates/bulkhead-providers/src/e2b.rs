//! E2B sandbox backend (https://e2b.dev) — hosted microVM provider.
//!
//! Implemented against their HTTP API (requires `E2B_API_KEY`):
//!   POST   /sandboxes                     create (templateID, timeoutMs)
//!   POST   /sandboxes/{id}/commands       execute a command
//!   DELETE /sandboxes/{id}                destroy
//!
//! Verified against a mock server in tests; the real API is called with the
//! same code path.

use bulkhead_core::action::Action;
use bulkhead_core::error::{BulkheadError, Result};

use crate::{ExecResult, SandboxProvider, SandboxSpec, shell_command};

#[derive(Debug, Clone)]
pub struct E2bProvider {
    pub api_key: String,
    pub base_url: String,
}

impl E2bProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.e2b.dev".into(),
        }
    }
}

impl SandboxProvider for E2bProvider {
    /// The E2B sandbox id.
    type Session = String;

    fn start(&self, spec: &SandboxSpec) -> Result<Self::Session> {
        let template = spec.image.clone().unwrap_or_else(|| "base".to_string());
        let body = serde_json::json!({ "templateID": template, "timeoutMs": 300_000 });
        let response = ureq::post(&format!("{}/sandboxes", self.base_url))
            .set("X-API-Key", &self.api_key)
            .send_json(body)
            .map_err(|e| BulkheadError::Provider(format!("e2b create sandbox: {e}")))?;
        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| BulkheadError::Provider(format!("e2b create sandbox response: {e}")))?;
        json.get("sandboxID")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| BulkheadError::Provider("e2b response missing sandboxID".into()))
    }

    fn exec(&self, session: &Self::Session, action: &Action) -> Result<ExecResult> {
        let command = shell_command(action)?;
        let body = serde_json::json!({ "cmd": command, "timeoutMs": 10_000 });
        let response = ureq::post(&format!("{}/sandboxes/{session}/commands", self.base_url))
            .set("X-API-Key", &self.api_key)
            .send_json(body)
            .map_err(|e| BulkheadError::Provider(format!("e2b exec: {e}")))?;
        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| BulkheadError::Provider(format!("e2b exec response: {e}")))?;
        Ok(ExecResult {
            exit_code: json
                .get("exitCode")
                .and_then(|v| v.as_i64())
                .map(|c| c as i32),
            stdout: json
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            stderr: json
                .get("stderr")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    fn stop(&self, session: Self::Session) -> Result<()> {
        ureq::delete(&format!("{}/sandboxes/{session}", self.base_url))
            .set("X-API-Key", &self.api_key)
            .call()
            .map_err(|e| BulkheadError::Provider(format!("e2b destroy sandbox: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::mock_server;

    #[test]
    fn full_lifecycle_against_mock_api() {
        let base = mock_server(3, |method, path, _body| match (method, path) {
            ("POST", "/sandboxes") => (201, r#"{"sandboxID":"sbx-123"}"#.to_string()),
            ("POST", "/sandboxes/sbx-123/commands") => (
                200,
                r#"{"exitCode":0,"stdout":"hello-from-e2b\n","stderr":""}"#.to_string(),
            ),
            ("DELETE", "/sandboxes/sbx-123") => (204, String::new()),
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        });

        let provider = E2bProvider {
            api_key: "test-key".into(),
            base_url: base.clone(),
        };

        let session = provider.start(&SandboxSpec::default()).unwrap();
        assert_eq!(session, "sbx-123");

        let action = Action::new(
            "agent-1",
            "shell.run",
            r#"{"command":"echo hello-from-e2b"}"#,
        );
        let result = provider.exec(&session, &action).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hello-from-e2b\n");

        provider.stop(session).unwrap();
    }

    #[test]
    fn non_shell_tools_refused() {
        let provider = E2bProvider::new("test-key");
        let action = Action::new("agent-1", "http.get", "{}");
        assert!(matches!(
            provider.exec(&"sbx".to_string(), &action),
            Err(BulkheadError::Provider(_))
        ));
    }
}

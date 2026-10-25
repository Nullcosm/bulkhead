//! Modal sandbox backend (https://modal.com) — serverless GPU/CPU provider.
//!
//! Implemented against their HTTP sandbox API (requires a Modal token pair):
//!   POST   /v1/sandboxes               create (image, timeout)
//!   POST   /v1/sandboxes/{id}/exec     execute a command
//!   DELETE /v1/sandboxes/{id}          destroy
//!
//! Auth via `Token-Id` / `Token-Secret` headers. Verified against a mock
//! server in tests; Modal's API surface has moved fast, so re-check the
//! endpoint shapes against current docs before a production rollout.

use bulkhead_core::action::Action;
use bulkhead_core::error::{BulkheadError, Result};

use crate::{ExecResult, SandboxProvider, SandboxSpec, shell_command};

#[derive(Debug, Clone)]
pub struct ModalProvider {
    pub token_id: String,
    pub token_secret: String,
    pub base_url: String,
}

impl ModalProvider {
    pub fn new(token_id: impl Into<String>, token_secret: impl Into<String>) -> Self {
        Self {
            token_id: token_id.into(),
            token_secret: token_secret.into(),
            base_url: "https://api.modal.com".into(),
        }
    }

    fn auth(&self, request: ureq::Request) -> ureq::Request {
        request
            .set("Token-Id", &self.token_id)
            .set("Token-Secret", &self.token_secret)
    }
}

impl SandboxProvider for ModalProvider {
    /// The Modal sandbox id.
    type Session = String;

    fn start(&self, spec: &SandboxSpec) -> Result<Self::Session> {
        let image = spec
            .image
            .clone()
            .unwrap_or_else(|| "python:3.12".to_string());
        let body = serde_json::json!({ "image": image, "timeout": 3600 });
        let response = self
            .auth(ureq::post(&format!("{}/v1/sandboxes", self.base_url)))
            .send_json(body)
            .map_err(|e| BulkheadError::Provider(format!("modal create sandbox: {e}")))?;
        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| BulkheadError::Provider(format!("modal create sandbox response: {e}")))?;
        json.get("sandbox_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| BulkheadError::Provider("modal response missing sandbox_id".into()))
    }

    fn exec(&self, session: &Self::Session, action: &Action) -> Result<ExecResult> {
        let command = shell_command(action)?;
        let body = serde_json::json!({ "cmd": command });
        let response = self
            .auth(ureq::post(&format!(
                "{}/v1/sandboxes/{session}/exec",
                self.base_url
            )))
            .send_json(body)
            .map_err(|e| BulkheadError::Provider(format!("modal exec: {e}")))?;
        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| BulkheadError::Provider(format!("modal exec response: {e}")))?;
        Ok(ExecResult {
            exit_code: json
                .get("exit_code")
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
        self.auth(ureq::delete(&format!(
            "{}/v1/sandboxes/{session}",
            self.base_url
        )))
        .call()
        .map_err(|e| BulkheadError::Provider(format!("modal destroy sandbox: {e}")))?;
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
            ("POST", "/v1/sandboxes") => (201, r#"{"sandbox_id":"sb-abc"}"#.to_string()),
            ("POST", "/v1/sandboxes/sb-abc/exec") => (
                200,
                r#"{"exit_code":0,"stdout":"hello-from-modal\n","stderr":""}"#.to_string(),
            ),
            ("DELETE", "/v1/sandboxes/sb-abc") => (204, String::new()),
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        });

        let provider = ModalProvider {
            token_id: "tid".into(),
            token_secret: "tsec".into(),
            base_url: base.clone(),
        };

        let session = provider.start(&SandboxSpec::default()).unwrap();
        assert_eq!(session, "sb-abc");

        let action = Action::new(
            "agent-1",
            "shell.run",
            r#"{"command":"echo hello-from-modal"}"#,
        );
        let result = provider.exec(&session, &action).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hello-from-modal\n");

        provider.stop(session).unwrap();
    }

    #[test]
    fn non_shell_tools_refused() {
        let provider = ModalProvider::new("tid", "tsec");
        let action = Action::new("agent-1", "db.query", "{}");
        assert!(matches!(
            provider.exec(&"sb".to_string(), &action),
            Err(BulkheadError::Provider(_))
        ));
    }
}

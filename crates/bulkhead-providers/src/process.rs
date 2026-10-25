//! Local process-level sandbox — the zero-infrastructure dev/demo backend.
//!
//! NOT an isolation boundary: actions run as plain child processes with a
//! scrubbed environment inside a per-session temp directory. Real isolation
//! ships on Firecracker (see docs/firecracker.md); this provider exists so
//! the control plane can be exercised end-to-end on a laptop, without a
//! hypervisor.
//!
//! Executable tools: only `shell.run`, whose params JSON must carry a
//! `command` string. Everything else is refused — the process sandbox is
//! deliberately tiny.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use bulkhead_core::action::Action;
use bulkhead_core::error::{BulkheadError, Result};

use crate::{ExecResult, SandboxProvider, SandboxSpec};

/// The process backend. Construction is free; state lives in the sessions.
pub struct ProcessProvider;

/// Handle to one running process environment (a private temp directory).
pub struct ProcessSession {
    /// Working directory unique to this session; removed on `stop`.
    pub workdir: PathBuf,
    /// Child process id, if `start` spawned an entrypoint.
    pub child: Option<u32>,
}

impl SandboxProvider for ProcessProvider {
    type Session = ProcessSession;

    fn start(&self, spec: &SandboxSpec) -> Result<Self::Session> {
        let workdir = session_dir();
        std::fs::create_dir_all(&workdir)
            .map_err(|e| BulkheadError::Provider(format!("create {}: {e}", workdir.display())))?;

        let child = match &spec.entrypoint {
            Some(entrypoint) => {
                let child = Command::new("sh")
                    .arg("-c")
                    .arg(entrypoint)
                    .current_dir(&workdir)
                    // Scrubbed environment: no host secrets leak in.
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("HOME", &workdir)
                    .spawn()
                    .map_err(|e| BulkheadError::Provider(format!("spawn entrypoint: {e}")))?;
                Some(child.id())
            }
            None => None,
        };

        Ok(ProcessSession { workdir, child })
    }

    fn exec(&self, session: &Self::Session, action: &Action) -> Result<ExecResult> {
        if action.tool != "shell.run" {
            return Err(BulkheadError::Provider(format!(
                "tool '{}' is not executable in the process sandbox (only shell.run)",
                action.tool
            )));
        }
        let params: serde_json::Value = serde_json::from_str(&action.params)
            .map_err(|e| BulkheadError::Provider(format!("params must be JSON: {e}")))?;
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                BulkheadError::Provider("shell.run params must include a 'command' string".into())
            })?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&session.workdir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &session.workdir)
            .output()
            .map_err(|e| BulkheadError::Provider(format!("execute: {e}")))?;

        Ok(ExecResult {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn stop(&self, session: Self::Session) -> Result<()> {
        if let Some(child) = session.child {
            let _ = Command::new("kill").arg(child.to_string()).status();
        }
        let _ = std::fs::remove_dir_all(&session.workdir);
        Ok(())
    }
}

fn session_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("bulkhead-proc-{}-{n}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> ProcessProvider {
        ProcessProvider
    }

    #[test]
    fn shell_run_executes_and_captures() {
        let p = provider();
        let session = p.start(&SandboxSpec::default()).unwrap();
        let action = Action::new(
            "agent-1",
            "shell.run",
            r#"{"command":"echo hello-bulkhead"}"#,
        );
        let result = p.exec(&session, &action).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "hello-bulkhead");
        p.stop(session).unwrap();
    }

    #[test]
    fn other_tools_are_refused() {
        let p = provider();
        let session = p.start(&SandboxSpec::default()).unwrap();
        let action = Action::new("agent-1", "http.get", "{}");
        let err = p.exec(&session, &action).unwrap_err();
        assert!(matches!(err, BulkheadError::Provider(_)));
        p.stop(session).unwrap();
    }

    #[test]
    fn missing_command_is_refused() {
        let p = provider();
        let session = p.start(&SandboxSpec::default()).unwrap();
        let action = Action::new("agent-1", "shell.run", "{}");
        assert!(p.exec(&session, &action).is_err());
        p.stop(session).unwrap();
    }

    #[test]
    fn host_environment_is_not_leaked() {
        // SAFETY: single-threaded test process; set/remove around the check.
        unsafe {
            std::env::set_var("BULKHEAD_TEST_SECRET", "do-not-leak");
        }
        let p = provider();
        let session = p.start(&SandboxSpec::default()).unwrap();
        let action = Action::new(
            "agent-1",
            "shell.run",
            r#"{"command":"echo secret=${BULKHEAD_TEST_SECRET:-absent}"}"#,
        );
        let result = p.exec(&session, &action).unwrap();
        assert_eq!(result.stdout.trim(), "secret=absent");
        p.stop(session).unwrap();
    }

    #[test]
    fn entrypoint_spawns_a_child() {
        let p = provider();
        let spec = SandboxSpec {
            entrypoint: Some("sleep 30".into()),
            ..SandboxSpec::default()
        };
        let session = p.start(&spec).unwrap();
        assert!(session.child.is_some());
        p.stop(session).unwrap();
    }
}

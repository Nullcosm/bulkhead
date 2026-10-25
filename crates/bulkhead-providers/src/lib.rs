//! Pluggable sandbox backends. Bulkhead is neutral: the same policy set governs
//! agents running on any provider.
//!
//! Implemented today: [`process::ProcessProvider`] (local, dev/demo),
//! [`e2b::E2bProvider`] (hosted microVMs), [`modal::ModalProvider`]
//! (serverless sandboxes). Firecracker runs on your own fleet via the same
//! [`SandboxProvider`] trait (see docs/firecracker.md).

pub mod e2b;
pub mod modal;
pub mod process;

use bulkhead_core::action::Action;
use bulkhead_core::error::{BulkheadError, Result};

/// What a provider needs to start an environment.
#[derive(Debug, Clone, Default)]
pub struct SandboxSpec {
    /// Container image / template (e.g. E2B template id, Modal image).
    pub image: Option<String>,
    pub cpu_millis: u32,
    pub mem_mb: u32,
    /// Command run at start (process provider only in v1).
    pub entrypoint: Option<String>,
}

/// Result of one executed action inside a sandbox.
#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Every backend implements this trait; the gateway only talks to the trait.
pub trait SandboxProvider {
    type Session;

    fn start(&self, spec: &SandboxSpec) -> Result<Self::Session>;
    fn exec(&self, session: &Self::Session, action: &Action) -> Result<ExecResult>;
    fn stop(&self, session: Self::Session) -> Result<()>;
}

/// The concrete backends, grouped so a gateway config can name one by string.
pub enum Provider {
    Process(process::ProcessProvider),
    E2b(e2b::E2bProvider),
    Modal(modal::ModalProvider),
}

impl Provider {
    /// Local process-level sandbox (development/demo backend).
    pub fn process() -> Self {
        Self::Process(process::ProcessProvider)
    }
}

/// Extract a shell command from an action. Providers execute `shell.run`
/// actions whose params carry a `command` string; everything else is refused.
pub(crate) fn shell_command(action: &Action) -> Result<String> {
    if action.tool != "shell.run" {
        return Err(BulkheadError::Provider(format!(
            "tool '{}' is not executable (providers run shell.run)",
            action.tool
        )));
    }
    let params: serde_json::Value = serde_json::from_str(&action.params)
        .map_err(|e| BulkheadError::Provider(format!("params must be JSON: {e}")))?;
    params
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            BulkheadError::Provider("shell.run params must include a 'command' string".into())
        })
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// Start a one-shot HTTP mock server. `handler(method, path, body) ->
    /// (status, json_body)`; accepts up to `max_requests` connections. Returns
    /// the base URL.
    ///
    /// Connections are handled **sequentially** in the accept loop — no
    /// per-connection threads. The provider tests issue their requests one at
    /// a time, and sequential handling removes the cross-platform races
    /// (macOS RST-on-close) that the threaded version hit.
    pub fn mock_server(
        max_requests: u32,
        handler: impl Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("mock addr");
        std::thread::spawn(move || {
            for _ in 0..max_requests {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };

                let (method, path, body) = {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone mock stream"));
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() {
                        continue;
                    }
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or("GET").to_string();
                    let path = parts.next().unwrap_or("/").to_string();
                    let mut content_length = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() {
                            break;
                        }
                        let line = line.trim_end().to_string();
                        if line.is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':')
                            && name.trim().eq_ignore_ascii_case("content-length")
                        {
                            content_length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; content_length];
                    if reader.read_exact(&mut body).is_err() {
                        continue;
                    }
                    (method, path, body)
                }; // reader dropped here: no open duplicate fd during the write

                let (status, response) = handler(&method, &path, &String::from_utf8_lossy(&body));
                let out = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    status_text(status),
                    response.len()
                );
                let _ = stream.write_all(out.as_bytes());
                // Half-close politely: send FIN before drop so clients never
                // see a reset mid-read (the macOS flake).
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        format!("http://{addr}")
    }

    fn status_text(code: u16) -> &'static str {
        match code {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Status",
        }
    }
}

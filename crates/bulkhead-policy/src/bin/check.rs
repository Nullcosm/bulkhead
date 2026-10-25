//! `bulkhead-policy` CLI — check and evaluate policy files without a gateway.
//!
//! ```text
//! bulkhead-policy check <file>
//! bulkhead-policy evaluate <file> --agent <id> --tool <tool> [--params <json>] [--resource <r>]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use bulkhead_core::action::{Action, Verdict};
use bulkhead_policy::Engine;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return ExitCode::from(2);
    };

    match command.as_str() {
        "check" => {
            let Some(file) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            check(&PathBuf::from(file))
        }
        "evaluate" => {
            let Some(file) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            evaluate(&PathBuf::from(file), args.collect())
        }
        "--help" | "-h" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            ExitCode::from(2)
        }
    }
}

fn check(file: &PathBuf) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", file.display());
            return ExitCode::from(1);
        }
    };
    match Engine::from_json(&bytes) {
        Ok(engine) => {
            println!("OK: {} rules", engine.rule_count());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn evaluate(file: &PathBuf, raw_args: Vec<String>) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", file.display());
            return ExitCode::from(1);
        }
    };
    let engine = match Engine::from_json(&bytes) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    let mut agent: Option<String> = None;
    let mut tool: Option<String> = None;
    let mut params = "{}".to_string();
    let mut resource: Option<String> = None;

    let mut it = raw_args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--agent" => agent = Some(it.next().expect("--agent needs a value")),
            "--tool" => tool = Some(it.next().expect("--tool needs a value")),
            "--params" => params = it.next().expect("--params needs a value"),
            "--resource" => resource = Some(it.next().expect("--resource needs a value")),
            other => {
                eprintln!("unknown argument: {other}");
                usage();
                return ExitCode::from(2);
            }
        }
    }

    let (Some(agent), Some(tool)) = (agent, tool) else {
        eprintln!("error: --agent and --tool are required");
        usage();
        return ExitCode::from(2);
    };

    let mut action = Action::new(agent, tool, params);
    if let Some(resource) = resource {
        action = action.with_resource(resource);
    }

    println!("{}", verdict_json(&engine.evaluate(&action)));
    ExitCode::SUCCESS
}

fn verdict_json(verdict: &Verdict) -> serde_json::Value {
    match verdict {
        Verdict::Allow => serde_json::json!({ "allow": true, "reason": "", "rule_id": null }),
        Verdict::Deny { reason, rule_id } => serde_json::json!({
            "allow": false,
            "reason": reason,
            "rule_id": rule_id
        }),
    }
}

fn usage() {
    eprintln!(
        "\
usage:
  bulkhead-policy check <file>
  bulkhead-policy evaluate <file> --agent <id> --tool <tool> [--params <json>] [--resource <r>]"
    );
}

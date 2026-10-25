//! `bulkhead-gateway` binary — the control plane entrypoint.
//!
//! Default mode serves the HTTP API v1 on `--listen`. `--demo` runs the
//! three-moment story against the live control plane; `--issue-token` prints
//! a credential for one agent (both useful for demos and scripts).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use bulkhead_core::agent::AgentId;
use bulkhead_gateway::server::serve;
use bulkhead_gateway::{Gateway, GatewayConfig};

const DEFAULT_IDENTITY_SECRET: &str = "dev-secret-change-me";

fn main() {
    eprintln!(
        "bulkhead-gateway v{} — the governance plane for AI agent fleets",
        env!("CARGO_PKG_VERSION")
    );

    let args = parse_args();

    match Gateway::load(&args.config) {
        Ok(mut gateway) => {
            if let Some(agent) = args.issue_token {
                let identity = gateway.issue_identity(AgentId::new(agent));
                println!("{}", identity.token);
                return;
            }

            println!(
                "loaded policy:  {} rules from {}",
                gateway.rule_count(),
                args.config.policy_file.display()
            );
            println!("audit trail:    {}", args.config.audit_file.display());

            for (agent, cents) in &args.budgets {
                if let Err(e) = gateway.set_budget(AgentId::new(agent.clone()), *cents) {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
                println!("budget set:     {} = {} cents", agent, cents);
            }

            if args.demo {
                if let Err(e) = gateway.demo() {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
                return;
            }

            if args.config.identity_secret == DEFAULT_IDENTITY_SECRET {
                eprintln!(
                    "warning: serving with the default identity secret — pass --identity-secret \
                     before any real deployment"
                );
            }
            println!("serving on:     {}", args.config.listen);
            println!("identity issuer: {}", args.config.issuer_name);
            println!(
                "default budget: {} cents/agent",
                args.config.default_budget_cents
            );

            let stop = Arc::new(AtomicBool::new(false));
            if let Err(e) = serve(gateway, &args.config.listen, stop, None) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

struct ParsedArgs {
    config: GatewayConfig,
    demo: bool,
    issue_token: Option<String>,
    budgets: Vec<(String, u64)>,
}

fn parse_args() -> ParsedArgs {
    let mut config = GatewayConfig {
        listen: "127.0.0.1:7373".into(),
        policy_file: PathBuf::from("policy.json"),
        default_budget_cents: 10_000,
        audit_file: PathBuf::from("audit.jsonl"),
        issuer_name: "bulkhead.local".into(),
        identity_secret: DEFAULT_IDENTITY_SECRET.into(),
        ledger_file: PathBuf::new(),
    };
    let mut demo = false;
    let mut issue_token = None;
    let mut budgets: Vec<(String, u64)> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => config.listen = args.next().expect("--listen needs a value"),
            "--policy" => {
                config.policy_file = PathBuf::from(args.next().expect("--policy needs a value"))
            }
            "--audit" => {
                config.audit_file = PathBuf::from(args.next().expect("--audit needs a value"))
            }
            "--default-budget-cents" => {
                config.default_budget_cents = args
                    .next()
                    .expect("--default-budget-cents needs a value")
                    .parse()
                    .expect("--default-budget-cents must be a number")
            }
            "--issuer" => config.issuer_name = args.next().expect("--issuer needs a value"),
            "--identity-secret" => {
                config.identity_secret = args.next().expect("--identity-secret needs a value")
            }
            "--ledger" => {
                config.ledger_file = PathBuf::from(args.next().expect("--ledger needs a value"))
            }
            "--issue-token" => {
                issue_token = Some(args.next().expect("--issue-token needs a value"))
            }
            "--budget" => {
                let spec = args.next().expect("--budget needs a value");
                let (agent, cents) = spec
                    .split_once(':')
                    .expect("--budget format is <agent>:<cents>");
                budgets.push((
                    agent.to_string(),
                    cents.parse().expect("--budget cents must be a number"),
                ));
            }
            "--demo" => demo = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
    }
    ParsedArgs {
        config,
        demo,
        issue_token,
        budgets,
    }
}

fn print_usage() {
    println!(
        "\
usage: bulkhead-gateway [OPTIONS]

Serves the HTTP API v1 (POST /v1/check, GET /healthz, /v1/policy, /v1/audit).

options:
  --listen <addr>              address to serve               [default: 127.0.0.1:7373]
  --policy <path>              v1 policy JSON file            [default: policy.json]
  --audit <path>               JSONL audit trail              [default: audit.jsonl]
  --default-budget-cents <n>   budget for agents without one  [default: 10000]
  --issuer <name>              identity issuer name           [default: bulkhead.local]
  --identity-secret <s>        HMAC signing secret            [default: dev value — set it!]
  --ledger <path>              budget ledger file (empty = memory)  [default: memory]
  --issue-token <agent>        print a short-lived token for <agent> and exit
  --budget <agent>:<cents>     set an agent budget at startup (repeatable)
  --demo                       run the three-moment demo and exit
  -h, --help                   print this help"
    );
}

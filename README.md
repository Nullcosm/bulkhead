# Bulkhead

[![CI](https://github.com/Nullcosm/bulkhead/actions/workflows/ci.yml/badge.svg)](https://github.com/Nullcosm/bulkhead/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![tag](https://img.shields.io/badge/version-v0.1.0-lightgrey.svg)](https://github.com/Nullcosm/bulkhead/releases/tag/v0.1.0)

<p align="center">
  <img src="assets/brand/logo.svg" width="128" alt="Bulkhead logo — a ship hull cut by the policy boundary">
</p>

> **The governance plane for AI agent fleets** — identity · policy · budget · audit.
> The thing that says **no** to agents — and proves it. Fail closed, ship fast.

**Bulkhead is the control plane for AI agents** — what Kubernetes + Vault + OPA did
for services, Bulkhead does for agents. Every tool call an agent makes is
adjudicated in one place: *who is this agent, is this action allowed, can it
afford it, and can an auditor see everything it did.*

> **Why not build a sandbox?** Firecracker, gVisor, and E2B already exist and
> are free. Bulkhead deliberately sits *above* them — one policy set, one audit
> trail, any backend (process sandbox, E2B, Modal, Firecracker). Neutrality is
> the moat.

## Status: MVP v0.1.0 (milestones M0–M6 complete)

| Piece | State |
|---|---|
| Domain model (`bulkhead-core`) | ✅ |
| Policy engine: globs, structured conditions, load-time validation (`bulkhead-policy`) + `bulkhead-policy` CLI | ✅ |
| Identity: short-lived HMAC-signed tokens (`bulkhead-identity`) | ✅ |
| HTTP API v1: `POST /v1/check`, `/healthz`, `/v1/policy`, `/v1/audit` (`bulkhead-gateway`) | ✅ |
| Budgets with kill switch, file-persisted (`bulkhead-ledger`) | ✅ |
| Append-only JSONL audit with id continuity across restarts (`bulkhead-audit`) | ✅ |
| Sandbox backends: process (local), E2B, Modal (`bulkhead-providers`) | ✅ |
| Python SDK `bulkhead-py`: fail-closed client + `@guard` decorator | ✅ |
| JavaScript SDK `bulkhead-js`: fail-closed client + `guardTool` | ✅ |
| Live three-moment demo (allowed / policy-blocked / budget-killed) | ✅ |

## Quickstart

```sh
cargo test --workspace            # all tests

# The demo, fully local (policy + budget + audit, no network):
cargo run -p bulkhead-gateway -- --demo --policy examples/policy.example.json

# The live demo (server + real tokens + SDK):
cargo run -p bulkhead-gateway -- \
  --policy examples/policy.example.json \
  --budget demo-agent:3 &
export BULKHEAD_ENDPOINT=http://127.0.0.1:7373
export BULKHEAD_TOKEN=$(cargo run -q -p bulkhead-gateway -- --policy examples/policy.example.json --issue-token demo-agent)
python3 examples/demo-agent/agent.py
```

Expected: the agent reads a URL (allowed), tries to write to prod (blocked by
`deny-prod-writes`), writes to staging (allowed by condition), and is
hard-stopped when its 3-cent budget dies — with every verdict on the audit
trail.

## Repository layout

```
bulkhead/
├── crates/
│   ├── bulkhead-core/        # shared domain types (Action, Verdict, AuditEvent, ...)
│   ├── bulkhead-policy/      # policy loading, validation, globs, conditions (+ CLI)
│   ├── bulkhead-identity/    # short-lived HMAC-signed agent credentials
│   ├── bulkhead-ledger/      # per-agent budgets, kill switch, file persistence
│   ├── bulkhead-audit/       # append-only JSONL audit trail
│   ├── bulkhead-providers/   # sandbox backends: process, E2B, Modal
│   └── bulkhead-gateway/     # adjudication loop + HTTP API v1
├── sdks/
│   ├── python/           # bulkhead-py (client + @guard)
│   └── js/               # bulkhead-js (client + guardTool)
├── examples/
│   ├── policy.example.json
│   └── demo-agent/       # the demo agent script
└── docs/
    ├── architecture.md   # data flow, decisions, API v1 spec
    ├── policy-model.md   # policy file format, conditions, validation
    ├── roadmap.md        # milestones M0–M6 + what's next
    ├── demo.md           # the three-moment demo story
    └── firecracker.md    # production isolation notes
```

## Docs

- [TESTING.md](TESTING.md) — four-level test ladder (demo → suite → live → break-it)
- [docs/architecture.md](docs/architecture.md) — how the pieces fit, API v1 spec, tech decisions
- [docs/policy-model.md](docs/policy-model.md) — policy file format, condition operators, validation
- [docs/roadmap.md](docs/roadmap.md) — what shipped and what's next
- [docs/demo.md](docs/demo.md) — the demo that is the sales pitch
- [docs/firecracker.md](docs/firecracker.md) — production sandbox isolation

## License

[Apache-2.0](LICENSE) · See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

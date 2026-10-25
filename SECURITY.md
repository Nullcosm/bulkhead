# Security policy

Bulkhead is a security product. Security of the project itself comes before
features, and reports are treated as top-priority.

## Reporting a vulnerability

**Do not open a public issue.**

- **GitHub private vulnerability reporting (preferred).** On the repository
  page: **Security tab → Report a vulnerability**. This opens a private
  conversation with the maintainers and supports coordinated disclosure
  (including public advisories once fixed). Reports arrive as GitHub
  notifications — no separate mailbox needed.
- **Email (optional).** When the project has its own domain or mailbox, add
  a `security@…` alias here as a fallback channel.

Include:

1. Affected version(s)
2. Reproduction steps or a minimal proof of concept
3. Impact — what an attacker gains (e.g. policy bypass, audit-tampering,
   budget bypass, token forgery, sandbox escape)

You will receive an acknowledgement within 72 hours and a status update
within 7 days.

## Scope

- The Rust workspace under `crates/`
- The SDKs under `sdks/`
- The demo agent under `examples/`

## Current trust boundaries (v0.1.0)

- **Identity tokens** are HMAC-SHA256 over a versioned payload
  (`bulkhead.v1.`). The gateway's `--identity-secret` IS the root credential:
  protect it like a database password, and never serve with the built-in
  default. Token verification is constant-time.
- **The process sandbox provider is not an isolation boundary.** It scrubs
  the environment and confines execution to a per-session temp directory,
  but untrusted code must run in Firecracker microVMs (see
  docs/firecracker.md), never in the process provider.
- **The audit trail is append-only by construction** (append-mode file +
  monotonic ids). Anything that could rewrite it is a reportable bug.
- **Fail-closed is the contract.** Policy errors, budget errors, missing
  endpoints, missing tokens — all stop the agent. Any path where a failure
  lets an action through ungoverned is a critical report.

## Known limitations (tracked, not hidden)

- HTTP API serves requests sequentially (post-M6 hardening item).
- Ledger persistence is file-based; multi-writer setups need SQLite.
- Asymmetric (ed25519) agent credentials are a roadmap item; the token
  format is versioned for a non-breaking migration.

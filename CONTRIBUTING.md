# Contributing

Thanks for considering it. Bulkhead is Apache-2.0 and built in public.

## Ground rules

1. **Open an issue first** for anything beyond a typo fix. Design decisions
   (policy format, API shape, audit semantics) are deliberate; discuss before
   you build.
2. **Tests are not optional.** Every behavioral change to `crates/*` ships
   with a test. Run `cargo test --workspace` before pushing.
3. **Deny-by-default is sacred.** No change may make Bulkhead fail *open* —
   policy, budget, or audit failures must stop the action.
4. **Keep the audit trail append-only.** Nothing ever deletes or rewrites an
   `AuditEvent`.

## Layout

- `crates/bulkhead-core` — domain types only; no I/O, no evaluation beyond
  first-match semantics.
- `crates/bulkhead-policy` — loading, validation, globs, and condition
  evaluation. A Cedar/Rego adapter would land here.
- `crates/bulkhead-identity` — credential issuance/verification.
- `crates/bulkhead-ledger` — budgets and the kill switch.
- `crates/bulkhead-audit` — append-only sinks.
- `crates/bulkhead-providers` — one module per sandbox backend behind the
  `SandboxProvider` trait.
- `crates/bulkhead-gateway` — the assembly point and the HTTP API v1.

## Commit style

Conventional, scoped to the crate: `feat(policy): ...`, `fix(audit): ...`,
`docs: ...`, `chore: ...`.

## Milestones

What to build next is tracked in [docs/roadmap.md](docs/roadmap.md). Claim a
milestone task by opening an issue before starting.

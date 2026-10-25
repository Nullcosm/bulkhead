# Roadmap

Every milestone through **M6 is complete** — Bulkhead is at MVP v0.1.0. What
remains is product hardening and the founder-side work from the playbook.

## Completed milestones

| Milestone | Definition of done | Evidence |
|---|---|---|
| **M0 — scaffold** | Workspace compiles; `--demo` runs the story | ✅ initial commit |
| **M1 — policy matcher** | Tool globbing (`http.*`), load-time rule validation, `bulkhead-policy check/evaluate` CLI | ✅ `crates/bulkhead-policy/src/glob.rs`, `src/bin/check.rs` |
| **M2 — HTTP interception point** | `POST /v1/check` adjudicates via policy → budget → audit; HMAC-signed tokens verified | ✅ `crates/bulkhead-gateway/src/server.rs` (full lifecycle test) |
| **M3 — structured conditions** | `condition` evaluated: `eq/ne/prefix/suffix/contains/in/exists` on `agent/tool/resource/params.*` | ✅ `crates/bulkhead-policy/src/conditions.rs` |
| **M4 — persistence** | Budgets survive restarts; audit ids stay monotonic | ✅ file-backed ledger + line-count seeding |
| **M5 — process sandbox** | Process-level execution with scrubbed env; demo agent runs live | ✅ `crates/bulkhead-providers/src/process.rs` + live e2e run |
| **M6 — provider connectors** | E2B + Modal HTTP clients behind `SandboxProvider`; mock-server tests | ✅ `src/e2b.rs`, `src/modal.rs` |

## Next (product hardening, in priority order)

1. **Concurrency** — the HTTP server handles requests sequentially; move the
   adjudication loop behind a thread pool (or tokio) before load testing.
2. **Red-team pass** — sandbox-escape and policy-bypass testing *before* any
   public launch claim; publish the report. This is the brand.
3. **ed25519 credentials** — the token format is versioned (`bulkhead.v1.`) so
   asymmetric per-agent keys slot in without breaking SDKs.
4. **Cedar or Rego adapter** — if customers outgrow the native condition ops,
   swap the evaluator behind `bulkhead-policy`'s engine; the format reserves the
   space.
5. **SDK framework wrappers** — LangChain `BaseTool` subclass, CrewAI
   decorator, OpenAI Agents SDK function wrapper (thin: the clients already
   do the work).
6. **SOC 2 program** — start the audit process; it gates enterprise pilots.
7. **Firecracker deployment** — see [firecracker.md](firecracker.md); the
   gateway code doesn't change, the provider implementation does.

## Non-goals (deliberate)

- No GPU scheduling — the giants can have it; Bulkhead governs whatever runs.
- No new policy language while the native condition ops suffice.
- No marketplace/registry before enterprise traction (year 2+ per the
  business plan).

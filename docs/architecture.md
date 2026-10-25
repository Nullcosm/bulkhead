# Architecture

## The problem in one picture

Agents today carry static API keys, call arbitrary tools, and leave no
trail. Bulkhead makes every action pass through one choke point:

```
 LLM / agent framework            SDK layer                CONTROL PLANE
┌────────────────────┐      ┌────────────────┐      ┌─────────────────────────────┐
│ LangChain          │─────▶│ bulkhead-py        │─────▶│ bulkhead-gateway (Rust)         │
│ CrewAI             │─────▶│ bulkhead-js        │      │                             │
│ OpenAI Agents SDK  │─────▶│ (fail closed)  │      │  1. identity  — who?        │
└────────────────────┘      └────────────────┘      │  2. policy    — allowed?    │
                                                    │  3. budget    — affordable? │
                                                    │  4. audit     — logged,     │
                                                    │                 always      │
                                                    └──────────┬──────────────────┘
                                                               ▼ verdict
                                              ┌──────────────────────────────────┐
                                              │ sandbox providers (pluggable)    │
                                              │ process │ E2B │ Modal │ Firecracker
                                              └──────────────────────────────────┘
```

## Adjudication order (the contract)

For **every** action:

1. **Identity** — the caller presents a short-lived HMAC-signed token
   (`bulkhead.v1.<payload>.<sig>`). Bad signature, untrusted issuer, expiry, or
   agent/token mismatch ⇒ `401`, nothing is evaluated.
2. **Policy** — first matching rule wins (tool globs × agent ids ×
   conditions); **no match = deny**. Bulkhead fails closed, always.
3. **Budget** — an allowed action must also fit the agent's budget. One cent
   over ⇒ kill switch; the action is denied *and audited*.
4. **Audit** — every adjudication, allowed or denied, is appended to the
   audit trail. The trail is append-only and never rewritten.

## Crate dependency graph

```
bulkhead-core          (domain types: Action, Verdict, PolicySet, AuditEvent, ...)
   ▲         ▲         ▲         ▲             ▲
   │         │         │         │             │
bulkhead-policy  bulkhead-identity  bulkhead-ledger  bulkhead-audit   bulkhead-providers
   │         │         │         │             │
   └─────────┴─────────┴────┬────┴─────────────┘
                            ▼
                      bulkhead-gateway      (adjudication loop + HTTP API v1)
```

`bulkhead-core` deliberately does no I/O and almost no evaluation: it is the data
model every other crate speaks. Crates only depend on `bulkhead-core`, never on
each other, so the policy engine can be swapped (Cedar/Rego) without touching
the ledger or audit trail.

## Key decisions (and why)

| Decision | Choice | Why |
|---|---|---|
| Language | Rust | A security product gets marketing value from memory safety, and it must be fast enough to sit on the hot path of every tool call |
| Policy semantics | Ordered rules, first-match-wins, deny-by-default | Auditable and explainable; matches how OPA/Rego behaves, so migration later is natural |
| Policy format | JSON, validated at load | Human-editable, diffable in git; invalid policies refuse to start the gateway |
| Conditions | Native evaluator (`eq/ne/prefix/suffix/contains/in/exists`) | Sufficient for v1; the format reserves space for a Cedar/Rego adapter |
| Identity | HMAC-SHA256, short-lived, versioned tokens | Zero key-management complexity for v1; `bulkhead.v1.` prefix makes ed25519 migration non-breaking |
| Ledger | File-backed JSON, atomic replace-on-write | Durable across restarts with zero new deps; SQLite when multi-writer arrives |
| Audit | JSONL, one `AuditEvent` per line, ids seeded from line count | Same shape ClickHouse/OpenSearch ingest will consume; append-only and restart-safe |
| Sandbox v0 | Process-level executor (dev/demo only) | The control plane is the product; real isolation ships on Firecracker (see firecracker.md) |
| HTTP server | `tiny_http`, sequential loop | Smallest correct thing; concurrency is the first post-M6 item |
| Dependencies | Only `serde`, `serde_json`, `hmac`, `sha2`, `base64`, `tiny_http`, `ureq` | A governance plane must minimize its own supply chain |

## HTTP API v1

The gateway serves on `--listen` (default `127.0.0.1:7373`).

### `POST /v1/check`

Request:

```json
{
  "agent": "agent-1",
  "token": "bulkhead.v1.eyJhZ2VudCI6...sig",
  "tool": "http.get",
  "params": "{\"url\":\"https://example.com\"}",
  "resource": "https://example.com"
}
```

Response `200` (always — a denial is a successful adjudication, not an error):

```json
{
  "verdict": { "allow": false, "reason": "denied by rule 'deny-prod-writes'", "rule_id": "deny-prod-writes" },
  "remaining_cents": null
}
```

- `400` = malformed request. `401` = bad/expired/mismatched token.
- `5xx` = Bulkhead broken — and because clients fail closed, a `5xx` **stops the
  agent**, it does not free it.
- `remaining_cents` is `null` on denial or when the ledger has no budget for
  the agent.

### Other endpoints

- `GET /healthz` → `{"status":"ok","version":"..."}`
- `GET /v1/policy` → `{"rules":[...]}` (the active policy, for transparency)
- `GET /v1/audit?agent=<id>` → `{"events":[...]}` (that agent's trail)

> The JSON DTOs of the HTTP API are the stable contract. Internal types
> (`Verdict` etc.) may serialize differently; the DTO mapping lives in
> `bulkhead-gateway/src/server.rs`, so internal refactors never break SDKs.

## Isolation honesty

- **The process provider is NOT an isolation boundary.** It scrubs the
  environment and confines execution to a per-session temp directory, but it
  exists so the control plane can be exercised end-to-end on a laptop.
- Production isolation ships on **Firecracker microVMs** (same machinery
  Vercel Sandbox and Fly Sprites use) behind the `SandboxProvider` trait —
  see [firecracker.md](firecracker.md). The gateway code does not change;
  the provider implementation does.
- The gateway itself is the trust anchor: it runs with no ambient
  credentials, and its audit sink is opened append-only.

## What to read next

- [policy-model.md](policy-model.md) — the policy file format
- [roadmap.md](roadmap.md) — milestone status and next steps
- [demo.md](demo.md) — the three-moment demo
- [firecracker.md](firecracker.md) — production isolation

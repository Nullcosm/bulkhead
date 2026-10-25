# Testing Bulkhead

Four levels, from "is it alive" to "try to break it". Each level lists the
commands and what passing looks like. Everything works offline after Cargo's
first dependency fetch.

**Prerequisites:** Rust (1.85+, edition 2024) · Python 3.10+ · `curl`.
Optional: Node 18+ for the JS SDK syntax check.

## Level 0 — the 20-second demo

No network, no server, nothing to configure:

```sh
cargo run -p bulkhead-gateway -- --demo --policy examples/policy.example.json
# or: make demo
```

**Passing looks like:**

```
  1. http.get   read a public URL              -> ALLOWED
  2. db.query   write to prod DB               -> DENIED: denied by rule 'deny-prod-writes'
  3. db.query   write to staging DB            -> ALLOWED
  4. http.get   more reads until budget dies   -> ALLOWED
  5. http.get   one read too many              -> DENIED: budget exhausted (kill switch)
```

…followed by the JSONL audit trail. If you see this, the whole control plane
(policy → budget → audit) works.

## Level 1 — the automated suite

```sh
cargo test --workspace                          # 54+ tests
cargo clippy --workspace --all-targets -- -D warnings   # must print nothing

PYTHONPATH=sdks/python python3 -m unittest discover -s sdks/python/tests
node --check sdks/js/src/index.js               # optional
```

**Passing:** every suite reports `ok`, clippy emits zero issues.

## Level 2 — the real product, live

Terminal 1 — the gateway:

```sh
cargo run -p bulkhead-gateway -- \
  --policy examples/policy.example.json \
  --budget demo-agent:3
```

Terminal 2 — act as an agent:

```sh
export BULKHEAD_ENDPOINT=http://127.0.0.1:7373
export BULKHEAD_TOKEN=$(cargo run -q -p bulkhead-gateway -- \
  --policy examples/policy.example.json --issue-token demo-agent)
python3 examples/demo-agent/agent.py
```

**Passing:** the same three-moment story, but every verdict now comes from the
running server over HTTP — the agent's tool functions execute only after the
gateway allows them.

> Without `BULKHEAD_ENDPOINT`/`BULKHEAD_TOKEN`, the agent script refuses to run tools
> (fail closed) and prints setup instructions — that refusal is also a test.

## Level 3 — the auditor's view

```sh
curl http://127.0.0.1:7373/healthz                    # {"status":"ok",...}
curl http://127.0.0.1:7373/v1/policy                  # the active rules
curl "http://127.0.0.1:7373/v1/audit?agent=demo-agent"  # every adjudication
```

**Passing:** health returns `ok`; `/v1/policy` shows the four rules;
`/v1/audit` shows every action — allowed and denied — with reasons and
`rule_id`s.

## Level 4 — try to break it

### 4a. Forged token → 401

```sh
curl -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:7373/v1/check \
  -d '{"agent":"demo-agent","token":"bulkhead.v1.forged.token","tool":"http.get","params":"{}"}'
```

**Passing:** `401` (a valid token returns `200` with a verdict).

### 4b. Broken policy must refuse to start

Create a policy with a duplicate rule id, then:

```sh
cargo run -p bulkhead-gateway -- --policy broken-policy.json --issue-token x
```

**Passing:** startup fails with `error: policy: duplicate rule id '...'`. A
broken policy file must never mean "no policy".

### 4c. Budget survives a restart

```sh
cargo run -p bulkhead-gateway -- --policy examples/policy.example.json \
  --ledger /tmp/bulkhead-ledger.json --budget demo-agent:1 &
```

Spend the 1-cent budget (one allowed action, then the next returns
`"budget exhausted (kill switch)"`), stop the gateway, start it again with
the same `--ledger`. **Passing:** the first action after restart is still
denied as exhausted — persistence is real.

### 4d. Policy CLI

```sh
cargo run -p bulkhead-policy --bin bulkhead-policy -- check examples/policy.example.json
cargo run -p bulkhead-policy --bin bulkhead-policy -- evaluate examples/policy.example.json \
  --agent demo-agent --tool db.query --params '{"sql":"DELETE FROM users"}' --resource prod.db
```

**Passing:** `OK: 4 rules`, then
`{"allow":false,"reason":"denied by rule 'deny-prod-writes',...}`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `Address already in use` | Pass `--listen 127.0.0.1:<other-port>` and update `BULKHEAD_ENDPOINT` |
| `ModuleNotFoundError: bulkhead_py` | The demo script adds `sdks/python` to `sys.path` itself; for your own code use `PYTHONPATH=sdks/python` or `pip install -e sdks/python` |
| "serving with the default identity secret" | Expected warning in dev; pass `--identity-secret` in any real deployment |
| `cargo: not found` | Install via [rustup](https://rustup.rs) |

## Security note

Level 2+ servers bind to localhost only. Everything in this ladder is safe to
run on a laptop; production deployment (Firecracker isolation, hardened
secrets) is covered in [docs/firecracker.md](docs/firecracker.md).

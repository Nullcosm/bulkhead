# The demo

The demo is not decoration — it is the sales pitch, compressed into 20
seconds. It shows the three moments that define Bulkhead:

1. **ALLOWED** — the agent reads a public URL. Policy says yes, budget has
   room, the action runs, the audit trail records it.
2. **BLOCKED** — the agent tries to write to the prod database. The policy
   condition on `resource` prefix `prod` says no *and says which rule said
   no* — while a staging write passes.
3. **KILLED** — the agent keeps working until its budget hits zero, and the
   next action is hard-stopped by the kill switch.

## Fully local (no network)

```sh
cargo run -p bulkhead-gateway -- --demo --policy examples/policy.example.json
```

Expected shape of the output:

```
bulkhead-gateway v0.1.0 — the governance plane for AI agent fleets
loaded policy:  4 rules from examples/policy.example.json
...
  1. http.get   read a public URL              -> ALLOWED
  2. db.query   write to prod DB               -> DENIED: denied by rule 'deny-prod-writes'
  3. db.query   write to staging DB            -> ALLOWED
  4. http.get   more reads until budget dies   -> ALLOWED
  5. http.get   one read too many              -> DENIED: budget exhausted (kill switch)
```

…followed by the real audit trail — every action, allowed or blocked,
timestamped and logged.

## Live, end-to-end (server + SDK + tokens)

```sh
# Terminal 1 — the gateway
cargo run -p bulkhead-gateway -- \
  --policy examples/policy.example.json \
  --budget demo-agent:3

# Terminal 2 — issue an identity, then run the agent
export BULKHEAD_ENDPOINT=http://127.0.0.1:7373
export BULKHEAD_TOKEN=$(cargo run -q -p bulkhead-gateway -- \
  --policy examples/policy.example.json --issue-token demo-agent)
python3 examples/demo-agent/agent.py
```

Output:

```
[bulkhead] adjudicating against the live gateway.

  read a public URL              -> ALLOWED [executed] GET https://example.com
  write to prod DB               -> BLOCKED: bulkhead blocked 'db.query': denied by rule 'deny-prod-writes' (rule deny-prod-writes)
  write to staging DB            -> ALLOWED [executed] INSERT INTO t VALUES (1)
  more reads until budget dies   -> ALLOWED [executed] GET https://api.example.com/a
  one read too many              -> BLOCKED: bulkhead blocked 'http.get': budget exhausted (kill switch)
```

Then ask the auditor's question:

```sh
curl "http://127.0.0.1:7373/v1/audit?agent=demo-agent"
```

Without `BULKHEAD_ENDPOINT`/`BULKHEAD_TOKEN` the agent script refuses to run tools
(fail closed) and prints setup instructions.

## The demo GIF

Record exactly this sequence — the verdicts scrolling past, ending on
`DENIED: budget exhausted (kill switch)`. If a viewer understands nothing
else, they understand that Bulkhead is the thing that says **no** to agents.
That is the brand.

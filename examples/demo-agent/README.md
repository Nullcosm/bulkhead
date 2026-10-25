# demo-agent

The demo agent from `docs/demo.md`. It walks the three Bulkhead moments —
allowed, policy-blocked, budget-killed.

## Run (script mode, works today)

```sh
python3 agent.py
```

## Run (live gateway, lands M2/M5)

```sh
cargo run -p bulkhead-gateway -- --policy examples/policy.example.json &
export BULKHEAD_ENDPOINT=http://127.0.0.1:7373
python3 agent.py
```

Until M2, the script refuses to run against an ungoverned environment: the
`bulkhead_py` client fails closed when `BULKHEAD_ENDPOINT` is missing.

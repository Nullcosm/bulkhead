# bulkhead-py

Python SDK for **Bulkhead**, the governance plane for AI agent fleets.

Wrap your agent's tools so every call is adjudicated by the gateway before it
executes:

```python
from bulkhead_py import GuardrailsClient

client = GuardrailsClient("agent-1")  # endpoint from $BULKHEAD_ENDPOINT
verdict = client.check_action("http.get", {"url": "https://example.com"})

if not verdict.allowed:
    print(f"blocked: {verdict.reason} (rule {verdict.rule_id})")
```

## Fail-closed by design

- No `BULKHEAD_ENDPOINT` configured → the client refuses to run actions.
- Gateway unreachable or `5xx` → raises `RuntimeError`; the agent stops.
  A broken guardrail must never mean "no guardrail".

## Status

The client implements the gateway's API v1 contract (`POST /v1/check`, see
`../../docs/architecture.md`). The server side of that contract lands in
milestone M2; until then the client is usable only for integration testing
and development against a mock.

## Development

```sh
python3 -m unittest discover -s tests
```

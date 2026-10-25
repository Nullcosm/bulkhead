# bulkhead-js

JavaScript SDK for **Bulkhead**, the governance plane for AI agent fleets.

```js
import { GuardrailsClient } from "bulkhead-js";

const client = new GuardrailsClient({ agentId: "agent-1" }); // endpoint from $BULKHEAD_ENDPOINT
const verdict = await client.checkAction("http.get", { url: "https://example.com" });

if (!verdict.allowed) {
  console.log(`blocked: ${verdict.reason} (rule ${verdict.ruleId})`);
}
```

## Fail-closed by design

- No `BULKHEAD_ENDPOINT` → the constructor throws; nothing runs ungoverned.
- Gateway unreachable or `5xx` → the promise rejects; the agent stops.

## Status

Implements the gateway's API v1 contract (`POST /v1/check`, see
`../../docs/architecture.md`); the server side lands in milestone M2.

## Development

```sh
node --check src/index.js
```

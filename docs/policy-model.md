# Policy model (v1)

A Bulkhead policy is one JSON document: an **ordered list of rules**. Evaluation
walks the list top to bottom; the **first matching rule wins**. If no rule
matches, the action is **denied** — Bulkhead is deny-by-default, structurally. It
cannot fail open.

## Format

```json
{
  "version": 1,
  "rules": [
    {
      "id": "deny-prod-writes",
      "effect": "deny",
      "tools": ["db.*"],
      "agents": [],
      "condition": { "field": "resource", "op": "prefix", "value": "prod" }
    },
    {
      "id": "allow-reads",
      "effect": "allow",
      "tools": ["http.*", "file.read"],
      "agents": [],
      "condition": null
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Stable rule identifier; appears in every denial reason and audit event |
| `effect` | `"allow"` or `"deny"` |
| `tools` | Tool patterns this rule applies to (`*` = any run, `?` = one char). Empty array = all tools |
| `agents` | Agent ids this rule applies to (exact match). Empty array = all agents |
| `condition` | Structured condition (below), or `null` for none |

A rule **matches** when `tools` × `agents` × `condition` all match. Unknown
fields are tolerated (forward compatibility); unknown values are not — see
validation.

## Conditions

The `condition` object narrows a rule to actions whose extracted field value
satisfies `op` against `value`:

| Operator | Meaning | `value` |
|---|---|---|
| `eq` | equal (string or number) | scalar |
| `ne` | not equal | scalar |
| `prefix` | string starts with | string |
| `suffix` | string ends with | string |
| `contains` | string contains | string |
| `in` | field is one of | array of scalars |
| `exists` | field is present (optionally `value: true/false`) | bool (optional) |

Extractable fields: `agent`, `tool`, `resource`, or any dotted path into the
action's JSON params (`params.sql`, `params.count`, …). Missing fields never
match a condition (fail closed).

## Validation (at load time)

A policy that fails validation is a **load error** — the gateway refuses to
start ungoverned. Rejected:

- duplicate or empty rule ids
- empty tool patterns or agent ids
- unknown `effect`
- malformed conditions: unknown `op`, non-extractable `field`, missing
  `value` for ops that need one

Check any file with the CLI:

```sh
bulkhead-policy check policy.json
bulkhead-policy evaluate policy.json --agent demo-agent --tool db.query \
  --params '{"sql":"DELETE FROM users"}' --resource prod.db
```

## Semantics that are guaranteed

1. **First match wins.** No rule shadowing surprises.
2. **No match = deny**, with reason `no rule matched (deny by default)`.
3. **Denials carry `rule_id`** so an auditor can trace *why*, not just *what*.
4. **Policy load failure = gateway refuses to start.** A broken policy file
   must never mean "no policy".

## Roadmap of the format

- Conditions are evaluated natively today. If customers need richer logic,
  the engine swaps to a Cedar or Rego adapter without changing this file
  format (the field space is reserved).
- Signed, versioned policy bundles for regulated industries (EU AI Act pack,
  SOC 2 pack) are the enterprise revenue feature — post-M6.

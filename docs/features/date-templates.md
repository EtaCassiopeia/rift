---
layout: default
title: Date Templates
parent: Features
nav_order: 23
---

# Date Templates

Rift expands a small set of date tokens in **response bodies** at serve time, so a mock can return
"now" or a date relative to it without scripting.

---

## Tokens

| Token | Expands to |
|:------|:-----------|
| `{{NOW}}` | The current instant. |
| `{{DAYS+N}}` / `{{DAYS-N}}` | N days after / before now. |
| `{{MONTHS+N}}` / `{{MONTHS-N}}` | N months after / before now. |

Each renders as an **RFC 3339 / ISO 8601 timestamp**, e.g. `2026-07-01T16:54:03.803691+00:00`.
Tokens are expanded only in the response body (not headers). An offset that overflows the
representable date range is left unchanged rather than erroring. No other tokens (`UUID`, `RANDOM`,
…) are supported.

---

## Example — an issued/expiry token

```json
{
  "port": 4511,
  "protocol": "http",
  "stubs": [{
    "predicates": [{ "equals": { "path": "/token" } }],
    "responses": [{
      "is": {
        "statusCode": 200,
        "headers": { "Content-Type": "application/json" },
        "body": "{\"issued\":\"{{NOW}}\",\"expires\":\"{{DAYS+30}}\",\"renews\":\"{{MONTHS+12}}\"}"
      }
    }]
  }]
}
```

```bash
curl http://localhost:4511/token
# {"issued":"2026-07-01T16:54:03.80Z...","expires":"2026-07-31T...","renews":"2027-07-01T..."}
```

Date tokens are independent of Mountebank `${request.*}` request templates — both can appear in the
same body.

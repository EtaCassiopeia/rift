---
layout: default
title: Correlated Isolation (Spaces)
parent: Features
nav_order: 21
---

# Correlated Isolation (Spaces)

A **space** partitions one imposter's stubs and state by a correlation id (the *flow id*), so
parallel test runs sharing a port don't see each other's stubs, scenario state, or recorded
requests.

---

## How a request's flow id is resolved

`_rift.flowState.flowIdSource` decides which flow (space) a request belongs to:

| `flowIdSource` | Resolution |
|:---------------|:-----------|
| `"imposter_port"` (default) | The imposter port — one shared space. |
| `"header:X-Mock-Space"` | The value of that request header (case-insensitive); falls back to the port if the header is absent. |

A stub's optional `space` field scopes it to one flow id. Stubs **without** a `space` are global and
match any caller. Space-scoped stubs are considered only when the request's resolved flow id equals
their `space`.

---

## Example — one port, isolated tenants

```json
{
  "port": 4510,
  "protocol": "http",
  "recordRequests": true,
  "_rift": {
    "flowState": {
      "backend": "inmemory",
      "ttlSeconds": 300,
      "flowIdSource": "header:X-Mock-Space"
    }
  },
  "stubs": [
    {
      "space": "alice",
      "predicates": [{ "equals": { "path": "/data" } }],
      "responses": [{ "is": { "statusCode": 200, "body": { "owner": "alice" } } }]
    },
    {
      "space": "bob",
      "predicates": [{ "equals": { "path": "/data" } }],
      "responses": [{ "is": { "statusCode": 200, "body": { "owner": "bob" } } }]
    },
    {
      "predicates": [{ "equals": { "path": "/health" } }],
      "responses": [{ "is": { "statusCode": 200, "body": "OK" } }]
    }
  ]
}
```

```bash
curl -H 'X-Mock-Space: alice' http://localhost:4510/data   # {"owner":"alice"}
curl -H 'X-Mock-Space: bob'   http://localhost:4510/data   # {"owner":"bob"}
curl http://localhost:4510/health                          # OK  (global stub)
```

---

## Managing spaces at runtime

Instead of declaring `space` inline, you can add stubs to a space through the admin API and tear the
whole space down in one call (its scoped stubs, recorded requests, and scenario state):

```bash
curl -X POST http://localhost:2525/imposters/4510/spaces/alice/stubs \
  -d '{"predicates":[{"equals":{"path":"/data"}}],"responses":[{"is":{"statusCode":200,"body":"scoped"}}]}'

curl http://localhost:2525/imposters/4510/spaces/alice        # inspect the space
curl -X DELETE http://localhost:2525/imposters/4510/spaces/alice   # teardown
```

Spaces build on the same store as [Flow State]({{ site.baseurl }}/features/flow-state/) and
[Scenarios]({{ site.baseurl }}/features/scenarios/), which are likewise partitioned by flow id.

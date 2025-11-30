---
layout: default
title: Fault Injection
parent: Features
nav_order: 1
---

# Fault Injection

Rift enables fault injection for chaos engineering and resilience testing.

---

## Mountebank Behaviors

### Latency with wait Behavior

```json
{
  "stubs": [{
    "predicates": [{ "equals": { "path": "/api/slow" } }],
    "responses": [{
      "is": { "statusCode": 200, "body": "OK" },
      "_behaviors": { "wait": 2000 }
    }]
  }]
}
```

### Random Latency

```json
{
  "_behaviors": {
    "wait": {
      "inject": "function() { return Math.floor(Math.random() * 1000) + 500; }"
    }
  }
}
```

### Error Responses

```json
{
  "stubs": [{
    "predicates": [{ "equals": { "path": "/api/error" } }],
    "responses": [{
      "is": {
        "statusCode": 500,
        "body": { "error": "Internal Server Error" }
      }
    }]
  }]
}
```

### Probabilistic Errors with Injection

```json
{
  "stubs": [{
    "responses": [{
      "inject": "function(request, state, logger) { \
        if (Math.random() < 0.1) { \
          return { statusCode: 500, body: 'Random failure' }; \
        } \
        return { statusCode: 200, body: 'Success' }; \
      }"
    }]
  }]
}
```

---

## Rift Extensions (`_rift.fault`)

### Probabilistic Latency

```json
{
  "port": 4545,
  "protocol": "http",
  "stubs": [{
    "predicates": [{ "startsWith": { "path": "/api" } }],
    "responses": [{
      "is": { "statusCode": 200, "body": "OK" },
      "_rift": {
        "fault": {
          "latency": {
            "probability": 0.3,
            "minMs": 100,
            "maxMs": 500
          }
        }
      }
    }]
  }]
}
```

### Probabilistic Errors

```json
{
  "stubs": [{
    "predicates": [{ "equals": { "method": "POST" } }],
    "responses": [{
      "is": { "statusCode": 200, "body": "OK" },
      "_rift": {
        "fault": {
          "error": {
            "probability": 0.1,
            "status": 503,
            "body": "{\"error\": \"Service Unavailable\"}",
            "headers": {
              "Retry-After": "30"
            }
          }
        }
      }
    }]
  }]
}
```

### Combined Faults

Apply both latency and errors:

```json
{
  "responses": [{
    "is": { "statusCode": 200, "body": "OK" },
    "_rift": {
      "fault": {
        "latency": {
          "probability": 0.5,
          "minMs": 200,
          "maxMs": 1000
        },
        "error": {
          "probability": 0.05,
          "status": 500
        }
      }
    }
  }]
}
```

### TCP Faults

Simulate network-level failures:

```json
{
  "_rift": {
    "fault": {
      "tcp": {
        "probability": 0.05,
        "type": "reset"
      }
    }
  }
}
```

TCP fault types:
- `reset` - RST packet (connection reset)
- `timeout` - Connection timeout
- `close` - Close connection without response

---

## Scripted Faults

### Rhai Script

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {"backend": "inmemory"}
  },
  "stubs": [{
    "responses": [{
      "_rift": {
        "script": {
          "engine": "rhai",
          "code": "let count = flow.get(\"counter\").unwrap_or(0); flow.set(\"counter\", count + 1); if count % 5 == 4 { #{ error_status: 500, error_body: \"Periodic failure\", inject: true } } else if request.path.contains(\"slow\") { #{ latency_ms: rand(100, 500), inject: true } } else { #{ inject: false } }"
        }
      }
    }]
  }]
}
```

### Time-Based Faults

```json
{
  "_rift": {
    "script": {
      "engine": "rhai",
      "code": "let hour = timestamp().hour(); if hour >= 9 && hour <= 17 { #{ error_probability: 0.1, inject: true } } else { #{ inject: false } }"
    }
  }
}
```

---

## Use Cases

### Testing Timeout Handling

```json
{
  "predicates": [{ "equals": { "path": "/api/external-service" } }],
  "responses": [{
    "is": { "statusCode": 200, "body": "OK" },
    "_rift": {
      "fault": {
        "latency": {
          "probability": 1.0,
          "ms": 35000
        }
      }
    }
  }]
}
```

### Testing Retry Logic

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {"backend": "inmemory"}
  },
  "stubs": [{
    "responses": [{
      "_rift": {
        "script": {
          "engine": "rhai",
          "code": "let key = \"attempt:\" + request.headers[\"X-Request-Id\"]; let attempts = flow.get(key).unwrap_or(0); flow.set(key, attempts + 1); flow.expire(key, 60); if attempts < 2 { #{ error_status: 503, inject: true } } else { #{ inject: false } }"
        }
      }
    }]
  }]
}
```

### Testing Circuit Breaker

```json
{
  "_rift": {
    "flowState": {"backend": "inmemory"},
    "script": {
      "engine": "rhai",
      "code": "let failures = flow.get(\"failure_count\").unwrap_or(0); if rand() < 0.5 { flow.set(\"failure_count\", failures + 1); #{ error_status: 500, inject: true } } else { flow.set(\"failure_count\", 0); #{ inject: false } }"
    }
  }
}
```

---

## Best Practices

1. **Start with low probability** - Begin at 1-5% and increase gradually
2. **Use specific matches** - Target specific endpoints, not all traffic
3. **Add identifiers** - Include headers to identify injected faults
4. **Monitor metrics** - Track fault injection rate and impact
5. **Test in staging first** - Validate fault scenarios before production
6. **Document scenarios** - Keep a runbook of chaos experiments

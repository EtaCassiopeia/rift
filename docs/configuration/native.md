---
layout: default
title: Rift Extensions (_rift namespace)
parent: Configuration
nav_order: 2
---

# Rift Extensions (`_rift` Namespace)

Rift extends Mountebank's JSON configuration with advanced features through the `_rift` namespace. This allows you to use Mountebank-compatible configurations while adding Rift-specific capabilities.

---

## Overview

The `_rift` namespace can be used at two levels:

1. **Imposter level** (`_rift`): For imposter-wide settings like flow state
2. **Response level** (`_rift`): For response-specific features like fault injection and scripting

---

## Flow State

Enable stateful testing scenarios with flow state:

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {
      "backend": "inmemory",
      "ttlSeconds": 300
    }
  },
  "stubs": [{
    "responses": [{
      "inject": "function(request, state) { state.count = (state.count || 0) + 1; return { statusCode: 200, body: 'Count: ' + state.count }; }"
    }]
  }]
}
```

### Flow State Backends

| Backend | Description | Use Case |
|:--------|:------------|:---------|
| `inmemory` | In-process storage (default) | Single instance, testing |
| `redis` | Redis-backed distributed storage | Multi-instance, production |

### Configuration Options

| Option | Type | Default | Description |
|:-------|:-----|:--------|:------------|
| `backend` | string | `"inmemory"` | Storage backend: inmemory or redis |
| `ttlSeconds` | integer | `300` | Time-to-live for state entries (5 minutes) |
| `redis` | object | - | Redis-specific configuration (required for redis backend) |

### Redis Configuration

When using `redis` backend:

```json
"_rift": {
  "flowState": {
    "backend": "redis",
    "ttlSeconds": 600,
    "redis": {
      "url": "redis://localhost:6379",
      "poolSize": 10,
      "keyPrefix": "rift:"
    }
  }
}
```

| Option | Type | Default | Description |
|:-------|:-----|:--------|:------------|
| `url` | string | required | Redis connection URL |
| `poolSize` | integer | `10` | Connection pool size |
| `keyPrefix` | string | `"rift:"` | Prefix for all keys (namespace isolation) |

**Connection URL formats:**

```bash
# Basic
redis://localhost:6379

# With password
redis://:password@localhost:6379

# With database selection
redis://localhost:6379/0

# TLS connection
rediss://localhost:6379

# Sentinel
redis+sentinel://localhost:26379/mymaster
```

**Key isolation example:**

```json
{
  "flowState": {
    "backend": "redis",
    "redis": {
      "url": "redis://localhost:6379",
      "keyPrefix": "rift:staging:"
    }
  }
}
```

This prefixes all keys with `rift:staging:` to isolate test environments.

### Enabling Redis Backend

Redis support requires building with the `redis-backend` feature:

```bash
# Build with Redis support
cargo build --release --features redis-backend

# Run with Redis backend
rift-http-proxy --configfile imposters.json
```

---

## Fault Injection

Add probabilistic fault injection to responses:

### Latency Faults

```json
{
  "is": {"statusCode": 200, "body": "OK"},
  "_rift": {
    "fault": {
      "latency": {
        "probability": 0.3,
        "minMs": 100,
        "maxMs": 500
      }
    }
  }
}
```

Or with fixed delay:

```json
"_rift": {
  "fault": {
    "latency": {
      "probability": 1.0,
      "ms": 200
    }
  }
}
```

### Error Faults

```json
{
  "is": {"statusCode": 200, "body": "OK"},
  "_rift": {
    "fault": {
      "error": {
        "probability": 0.1,
        "status": 503,
        "body": "Service Unavailable",
        "headers": {
          "Retry-After": "60"
        }
      }
    }
  }
}
```

### TCP Faults

```json
"_rift": {
  "fault": {
    "tcp": {
      "probability": 0.05,
      "type": "reset"
    }
  }
}
```

TCP fault types:
- `reset`: RST packet (connection reset)
- `timeout`: Connection timeout
- `close`: Close connection without response

---

## Scripting

Use multi-engine scripting for complex response logic:

### Rhai (Built-in)

```json
{
  "_rift": {
    "script": {
      "engine": "rhai",
      "code": "let count = flow.get('count').unwrap_or(0); flow.set('count', count + 1); #{ statusCode: 200, body: `Count: ${count + 1}` }"
    }
  }
}
```

### Lua (requires `--features lua`)

```json
{
  "_rift": {
    "script": {
      "engine": "lua",
      "code": "local count = flow:get('count') or 0; flow:set('count', count + 1); return { statusCode = 200, body = 'Count: ' .. (count + 1) }"
    }
  }
}
```

### JavaScript (requires `--features javascript`)

```json
{
  "_rift": {
    "script": {
      "engine": "javascript",
      "code": "const count = flow.get('count') || 0; flow.set('count', count + 1); return { statusCode: 200, body: `Count: ${count + 1}` };"
    }
  }
}
```

---

## Complete Example

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {
      "backend": "inmemory",
      "ttlSeconds": 300
    }
  },
  "stubs": [
    {
      "predicates": [{"equals": {"path": "/api/users"}}],
      "responses": [{
        "is": {
          "statusCode": 200,
          "headers": {"Content-Type": "application/json"},
          "body": "{\"users\": []}"
        },
        "_rift": {
          "fault": {
            "latency": {
              "probability": 0.2,
              "minMs": 50,
              "maxMs": 200
            }
          }
        }
      }]
    },
    {
      "predicates": [{"equals": {"method": "POST", "path": "/api/orders"}}],
      "responses": [{
        "is": {"statusCode": 201, "body": "Created"},
        "_rift": {
          "fault": {
            "error": {
              "probability": 0.05,
              "status": 503,
              "body": "Service temporarily unavailable"
            }
          }
        }
      }]
    },
    {
      "predicates": [{"equals": {"path": "/api/counter"}}],
      "responses": [{
        "_rift": {
          "script": {
            "engine": "rhai",
            "code": "let count = flow.get('requests').unwrap_or(0) + 1; flow.set('requests', count); #{ statusCode: 200, body: `Request #${count}` }"
          }
        }
      }]
    }
  ]
}
```

---

## Combining with Mountebank Features

`_rift` extensions work alongside standard Mountebank features:

```json
{
  "is": {
    "statusCode": 200,
    "body": "Hello"
  },
  "_behaviors": {
    "wait": 50,
    "decorate": "function(request, response) { response.body += ' World'; }"
  },
  "_rift": {
    "fault": {
      "latency": {
        "probability": 0.1,
        "ms": 100
      }
    }
  }
}
```

Both `_behaviors.wait` and `_rift.fault.latency` will be applied.

---

## See Also

- [Mountebank Compatibility](mountebank.md) - Standard Mountebank configuration
- [Fault Injection](../features/fault-injection.md) - Detailed fault injection documentation
- [Scripting](../features/scripting.md) - Scripting engine documentation

---
layout: default
title: Native Rift Format
parent: Configuration
nav_order: 2
---

# Native Rift Configuration Format

The native YAML format provides extended chaos engineering features beyond Mountebank compatibility.

---

## When to Use Native Format

Use the native format when you need:
- Probabilistic fault injection
- Sidecar or reverse proxy deployment modes
- Per-upstream routing rules
- Advanced scripting (Rhai, Lua)
- Flow state management

---

## Basic Structure

```yaml
# Deployment mode: sidecar or reverse_proxy
mode: sidecar

# Listener configuration
listen:
  port: 8080
  host: "0.0.0.0"

# Target service (sidecar mode)
upstream:
  host: "localhost"
  port: 8081

# Fault injection rules
rules:
  - id: "rule-name"
    match: {...}
    fault: {...}
```

---

## Deployment Modes

### Sidecar Mode

One proxy per service, injecting faults for a single upstream:

```yaml
mode: sidecar

listen:
  port: 8080

upstream:
  host: "app-service"
  port: 8081

rules:
  - id: "inject-latency"
    match:
      path:
        prefix: "/api"
    fault:
      latency:
        probability: 0.1
        min_ms: 100
        max_ms: 500
```

### Reverse Proxy Mode

Single proxy routing to multiple upstreams:

```yaml
mode: reverse_proxy

listen:
  port: 8080

upstreams:
  - name: "user-service"
    host: "users.internal"
    port: 8081
    routes:
      - prefix: "/api/users"

  - name: "order-service"
    host: "orders.internal"
    port: 8082
    routes:
      - prefix: "/api/orders"

rules:
  - id: "user-service-fault"
    upstream: "user-service"
    fault:
      error:
        probability: 0.05
        status_code: 503
```

---

## Listen Configuration

```yaml
listen:
  port: 8080
  host: "0.0.0.0"  # Bind address (default: 0.0.0.0)

  # TLS configuration
  tls:
    cert_file: "/path/to/cert.pem"
    key_file: "/path/to/key.pem"
```

---

## Upstream Configuration

### Single Upstream (Sidecar)

```yaml
upstream:
  host: "backend-service"
  port: 8081

  # Optional TLS to upstream
  tls:
    enabled: true
    verify: true  # Verify server certificate
    ca_file: "/path/to/ca.pem"
```

### Multiple Upstreams (Reverse Proxy)

```yaml
upstreams:
  - name: "service-a"
    host: "service-a.internal"
    port: 8081
    routes:
      - prefix: "/api/a"
      - exact: "/health/a"

  - name: "service-b"
    host: "service-b.internal"
    port: 8082
    routes:
      - prefix: "/api/b"
    tls:
      enabled: true
```

---

## Rules Configuration

### Match Conditions

```yaml
rules:
  - id: "my-rule"
    match:
      # HTTP methods
      methods: ["GET", "POST"]

      # Path matching
      path:
        prefix: "/api"
        # OR exact: "/api/endpoint"
        # OR regex: "/api/users/\\d+"

      # Header matching
      headers:
        X-Debug: "true"
        Content-Type:
          contains: "json"

      # Query parameter matching
      query:
        version: "2"
```

### Fault Injection

```yaml
rules:
  - id: "latency-fault"
    match:
      path:
        prefix: "/api"
    fault:
      latency:
        probability: 0.3    # 30% of requests
        min_ms: 100
        max_ms: 1000

  - id: "error-fault"
    match:
      methods: ["POST"]
    fault:
      error:
        probability: 0.1    # 10% of requests
        status_code: 500
        body: '{"error": "Injected failure"}'
        headers:
          X-Fault-Injected: "true"
```

### Combined Faults

```yaml
rules:
  - id: "combined-fault"
    match:
      path:
        prefix: "/critical"
    fault:
      # Apply both latency and error
      latency:
        probability: 0.5
        min_ms: 200
        max_ms: 500
      error:
        probability: 0.1
        status_code: 503
```

---

## Scripting

### Rhai Scripts

```yaml
rules:
  - id: "scripted-fault"
    match:
      path:
        prefix: "/api"
    fault:
      script:
        engine: rhai
        code: |
          // Access request details
          let path = request.path;
          let method = request.method;

          // Conditional fault injection
          if path.contains("admin") {
            #{
              latency_ms: 500,
              inject: true
            }
          } else {
            #{
              inject: false
            }
          }
```

### Lua Scripts

```yaml
rules:
  - id: "lua-fault"
    match:
      path:
        prefix: "/api"
    fault:
      script:
        engine: lua
        code: |
          local path = request.path
          if string.find(path, "slow") then
            return { latency_ms = 1000, inject = true }
          end
          return { inject = false }
```

---

## Flow State

Track state across requests for complex scenarios:

```yaml
flow_state:
  backend: memory  # or "redis"

  # Redis configuration (if backend: redis)
  redis:
    url: "redis://localhost:6379"
    prefix: "rift:"

rules:
  - id: "stateful-fault"
    fault:
      script:
        engine: rhai
        code: |
          // Increment request counter
          let count = flow.get("request_count").unwrap_or(0);
          flow.set("request_count", count + 1);

          // Fail every 5th request
          if count % 5 == 4 {
            #{
              error_status: 500,
              inject: true
            }
          } else {
            #{
              inject: false
            }
          }
```

---

## Metrics

```yaml
metrics:
  enabled: true
  port: 9090
  path: "/metrics"
```

Exposes Prometheus metrics:
- `rift_requests_total` - Request count by path, method, status
- `rift_request_duration_seconds` - Request latency histogram
- `rift_faults_injected_total` - Fault injection count
- `rift_script_execution_seconds` - Script execution time

---

## Complete Example

```yaml
mode: reverse_proxy

listen:
  port: 8080
  host: "0.0.0.0"

upstreams:
  - name: "user-service"
    host: "users.svc.cluster.local"
    port: 8080
    routes:
      - prefix: "/api/users"
      - exact: "/api/me"

  - name: "order-service"
    host: "orders.svc.cluster.local"
    port: 8080
    routes:
      - prefix: "/api/orders"

flow_state:
  backend: redis
  redis:
    url: "redis://redis.svc.cluster.local:6379"

metrics:
  enabled: true
  port: 9090

rules:
  # Add latency to user service
  - id: "user-service-latency"
    upstream: "user-service"
    match:
      methods: ["GET"]
    fault:
      latency:
        probability: 0.2
        min_ms: 50
        max_ms: 200

  # Inject errors on order creation
  - id: "order-creation-errors"
    upstream: "order-service"
    match:
      methods: ["POST"]
      path:
        exact: "/api/orders"
    fault:
      error:
        probability: 0.05
        status_code: 503
        body: '{"error": "Service temporarily unavailable"}'

  # Scripted fault for rate limiting simulation
  - id: "rate-limit-simulation"
    match:
      path:
        prefix: "/api"
    fault:
      script:
        engine: rhai
        code: |
          let key = "rate:" + request.headers["X-User-Id"];
          let count = flow.get(key).unwrap_or(0);
          flow.set(key, count + 1);
          flow.expire(key, 60);  // Reset after 60 seconds

          if count > 100 {
            #{
              error_status: 429,
              error_body: `{"error": "Rate limit exceeded"}`,
              inject: true
            }
          } else {
            #{ inject: false }
          }
```

---

## Configuration Validation

Rift validates configuration at startup:

```bash
# Test configuration
./rift-http-proxy --validate config.yaml

# Run with config
./rift-http-proxy config.yaml
```

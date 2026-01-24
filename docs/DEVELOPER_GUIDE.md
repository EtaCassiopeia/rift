# Rift Developer Guide

This guide helps new contributors understand Rift's architecture and start contributing quickly.

## What is Rift?

Rift is a high-performance, **Mountebank-compatible** HTTP/HTTPS mock server written in Rust. It serves as a drop-in replacement for Mountebank with identical REST API and configuration format, delivering 2-250x better performance.

**Key Use Cases:**
- Service virtualization for integration testing
- Chaos engineering and fault injection
- API mocking during development
- Contract testing

## Project Structure

```
rift/
├── crates/
│   ├── rift-http-proxy/    # Core mock server (main crate)
│   ├── rift-lint/          # Configuration validator
│   └── rift-tui/           # Terminal UI
├── docs/                   # Documentation
├── examples/               # Example configurations
├── tests/                  # Integration tests & benchmarks
├── scripts/                # Build & deployment scripts
└── packages/               # Node.js wrapper
```

### Crate Overview

| Crate | Purpose |
|-------|---------|
| **rift-http-proxy** | Main server binary. Handles HTTP/HTTPS mocking, proxy mode, Admin API, predicates, behaviors, and scripting. |
| **rift-lint** | CLI tool to validate imposter JSON/YAML configurations before deployment. |
| **rift-tui** | Interactive terminal UI for managing imposters and stubs. |

## Core Concepts

### Imposters

An **imposter** is a mock server listening on a specific port. Each imposter has:
- A port number (user-specified or auto-assigned)
- A protocol (HTTP or HTTPS)
- One or more stubs defining request/response pairs
- Optional request recording

```json
{
  "port": 4545,
  "protocol": "http",
  "name": "User Service",
  "stubs": [...]
}
```

### Stubs

A **stub** defines a request matcher (predicates) and corresponding responses:

```json
{
  "predicates": [{ "equals": { "method": "GET", "path": "/users" } }],
  "responses": [{ "is": { "statusCode": 200, "body": {"users": []} } }]
}
```

### Predicates

Predicates match incoming requests. Supported operators:

| Operator | Description |
|----------|-------------|
| `equals` | Exact match |
| `deepEquals` | Deep structural equality |
| `contains` | Substring/partial match |
| `startsWith` / `endsWith` | Prefix/suffix match |
| `matches` | Regex pattern |
| `exists` | Field existence check |
| `jsonpath` | JSON path query |
| `xpath` | XML path query |
| `and` / `or` / `not` | Logical operators |

**Match fields:** method, path, query, headers, body

### Responses

Response types:
- **`is`** - Static response with statusCode, headers, body
- **`proxy`** - Forward request to upstream server
- **`inject`** - Dynamic response via scripting

### Behaviors

Behaviors modify responses:

| Behavior | Description |
|----------|-------------|
| `wait` | Add latency (fixed or random) |
| `repeat` | Repeat response N times before cycling |
| `copy` | Extract request fields into response |
| `lookup` | Query external CSV data |
| `decorate` | Script-based response transformation |
| `shellTransform` | External command transformation |

## Rift Extensions

Rift extends Mountebank with powerful features not available in the original. These are configured via the `_rift` field in imposter configurations.

### Fault Injection

Inject failures probabilistically to test resilience:

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "fault": {
      "latency": {
        "probability": 0.3,
        "min_ms": 100,
        "max_ms": 500
      },
      "error": {
        "probability": 0.1,
        "status": 503,
        "body": "{\"error\": \"Service Unavailable\"}",
        "headers": {"Retry-After": "30"}
      }
    }
  },
  "stubs": [...]
}
```

**Fault Types:**
| Type | Description |
|------|-------------|
| `latency` | Add random delay (min_ms to max_ms) with given probability |
| `error` | Return error response with status code, body, and headers |
| `tcp_fault` | TCP-level failures (connection reset, etc.) |

### Flow State (Stateful Testing)

Flow State enables **stateful mock behavior** by persisting data across HTTP requests. This is essential for testing scenarios where responses depend on previous interactions.

**Why Flow State?**
- Standard mocks are stateless - each request is independent
- Real services often have state (login sessions, counters, workflows)
- Flow State lets you simulate stateful behavior in your mocks

#### Key Concepts

- **flow_id** - A namespace for related state (e.g., user ID, session ID, test scenario)
- **key** - Individual data item within a flow (e.g., "login_count", "last_action")
- **TTL** - Time-to-live; state automatically expires after this period

#### Backend Configuration

**In-Memory Backend** (default):
```json
{
  "_rift": {
    "flowState": {
      "backend": "inmemory",
      "ttl_seconds": 300
    }
  }
}
```
- Fast, no external dependencies
- State lost on server restart
- Single-instance only (not shared across multiple Rift processes)

**Redis Backend** (distributed):
```json
{
  "_rift": {
    "flowState": {
      "backend": "redis",
      "ttl_seconds": 300,
      "redis": {
        "url": "redis://localhost:6379",
        "pool_size": 10,
        "key_prefix": "rift:"
      }
    }
  }
}
```
- State persists across restarts
- Shared across multiple Rift instances
- Compatible with Redis 6.x, 7.x, and Valkey

#### Flow Store API

Available in scripts via the `flow_store` object:

| Operation | Signature | Returns | Description |
|-----------|-----------|---------|-------------|
| `get` | `get(flow_id, key)` | Value or `()` | Retrieve stored value; returns unit `()` if not found |
| `set` | `set(flow_id, key, value)` | `bool` | Store any JSON-compatible value |
| `increment` | `increment(flow_id, key)` | `i64` | Atomic counter increment; initializes to 1 if not exists |
| `exists` | `exists(flow_id, key)` | `bool` | Check if key exists and is not expired |
| `delete` | `delete(flow_id, key)` | `bool` | Remove a key |
| `set_ttl` | `set_ttl(flow_id, ttl_secs)` | `bool` | Update TTL for all keys in a flow |

#### Practical Examples

**Rate Limiting by IP:**
```rhai
fn should_inject(request, flow_store) {
    let client_ip = request.headers["x-forwarded-for"];
    if client_ip == () { client_ip = "unknown"; }

    let count = flow_store.increment(client_ip, "requests");

    if count > 100 {
        return #{
            inject: true,
            fault: "error",
            status: 429,
            body: `{"error": "Rate limit exceeded", "count": ${count}}`,
            headers: #{ "Retry-After": "60" }
        };
    }

    #{ inject: false }
}
```

**Multi-Step Workflow (Login Required):**
```rhai
fn should_inject(request, flow_store) {
    let session_id = request.headers["x-session-id"];
    if session_id == () {
        return #{ inject: true, fault: "error", status: 401, body: "Missing session" };
    }

    // Check if user has logged in
    let is_authenticated = flow_store.get(session_id, "authenticated");

    if request.path == "/api/login" && request.method == "POST" {
        // Mark session as authenticated
        flow_store.set(session_id, "authenticated", true);
        return #{ inject: false };
    }

    if is_authenticated != true {
        return #{ inject: true, fault: "error", status: 401, body: "Not authenticated" };
    }

    #{ inject: false }
}
```

**Request Counting for Testing:**
```rhai
fn should_inject(request, flow_store) {
    // Track how many times each endpoint is called
    let count = flow_store.increment("metrics", request.path);

    // Fail on 3rd attempt (for retry testing)
    if count == 3 {
        return #{ inject: true, fault: "error", status: 500, body: "Temporary failure" };
    }

    #{ inject: false }
}
```

#### Implementation Details

- **Thread-safe**: All operations use appropriate locking (RwLock for in-memory, connection pooling for Redis)
- **Atomic increments**: The `increment` operation is atomic, safe for concurrent access
- **Lazy cleanup**: Expired keys are cleaned up opportunistically during writes
- **Key format**: Internal keys are prefixed as `flow:{flow_id}:{key}`

### Scripting

Dynamic behavior using embedded scripting engines. Scripts are configured at the **stub response level** using `_rift.script`.

#### Configuration

```json
{
  "stubs": [{
    "predicates": [{"equals": {"path": "/api/test"}}],
    "responses": [{
      "_rift": {
        "script": {
          "engine": "rhai",
          "code": "fn should_inject(request, flow_store) { #{inject: false} }"
        }
      }
    }]
  }]
}
```

**Fields:**
- `engine` - Script engine: `"rhai"` (default), `"lua"`, or `"javascript"`
- `code` - Inline script code (must define `should_inject` function)

#### Supported Engines

| Engine | Description |
|--------|-------------|
| `rhai` | Default. Rust-native, fast, safe sandboxed execution |
| `lua` | Lua 5.4 via mlua (feature-gated) |
| `javascript` | ECMAScript via Boa engine (feature-gated) |

#### Script Interface

Scripts must define a `should_inject(request, flow_store)` function:

**Request Object:**
```rhai
request.method      // "GET", "POST", etc.
request.path        // "/api/users/123"
request.headers     // Map: request.headers["content-type"]
request.query       // Map: request.query["page"]
request.pathParams  // Map: request.pathParams["id"]
request.body        // Parsed JSON body
```

**Return Value (Decision):**

Scripts must return a map indicating whether to inject a fault:

```rhai
// No fault - pass through to normal response
#{ inject: false }

// Inject error response
#{
    inject: true,
    fault: "error",
    status: 503,
    body: "{\"error\": \"Service unavailable\"}",
    headers: #{ "Content-Type": "application/json" }
}

// Inject latency only
#{
    inject: true,
    fault: "latency",
    duration_ms: 500
}
```

#### Complete Example

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {"backend": "inmemory", "ttlSeconds": 300}
  },
  "stubs": [{
    "responses": [{
      "_rift": {
        "script": {
          "engine": "rhai",
          "code": "fn should_inject(request, flow_store) { let count = flow_store.increment(\"demo\", \"hits\"); if count > 5 { #{inject: true, fault: \"error\", status: 429, body: `{\"error\":\"Rate limited\",\"count\":${count}}`} } else { #{inject: false} } }"
        }
      }
    }]
  }]
}
```

### Prometheus Metrics

Built-in observability via `/metrics` endpoint (default port 9090):

```bash
# Start with metrics enabled
cargo run -p rift-http-proxy -- --metrics-port 9090
```

**Key Metrics:**
- `rift_requests_total` - Total requests by method and status
- `rift_faults_injected_total` - Fault injection counts by type
- `rift_latency_injected_ms` - Histogram of injected latencies
- `rift_script_execution_duration_ms` - Script performance
- `rift_flow_state_ops_total` - Flow state operations
- `rift_proxy_request_duration_ms` - End-to-end request timing

### Multi-Upstream Routing

Route requests to different backends based on rules:

```json
{
  "_rift": {
    "routing": {
      "routes": [
        {
          "name": "api-v2",
          "match": {
            "path_prefix": "/api/v2",
            "headers": [{"name": "x-api-version", "value": "2"}]
          },
          "upstream": "backend-v2"
        },
        {
          "name": "default",
          "match": {"path_prefix": "/"},
          "upstream": "backend-v1"
        }
      ],
      "upstreams": {
        "backend-v1": {"url": "http://localhost:8001"},
        "backend-v2": {"url": "http://localhost:8002"}
      }
    }
  }
}
```

**Match Criteria:**
- `host` - Exact or wildcard (`*.example.com`) host matching
- `path_prefix` - Path starts with value
- `path_exact` - Exact path match
- `path_regex` - Regular expression match
- `headers` - Required header name/value pairs

Routes use first-match-wins ordering.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      Admin API (:2525)                       │
│              REST endpoints for imposter management          │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    ImposterManager                           │
│         Central registry for all running imposters           │
└─────────────────────────┬───────────────────────────────────┘
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
    ┌──────────┐    ┌──────────┐    ┌──────────┐
    │ Imposter │    │ Imposter │    │ Imposter │
    │  :4545   │    │  :4546   │    │  :4547   │
    └────┬─────┘    └────┬─────┘    └────┬─────┘
         │               │               │
         ▼               ▼               ▼
    ┌──────────────────────────────────────────┐
    │              Request Handler              │
    │  Predicate Matching → Response Building   │
    │         → Behavior Application            │
    └──────────────────────────────────────────┘
```

### Key Source Directories (`crates/rift-http-proxy/src/`)

| Directory | Purpose |
|-----------|---------|
| `admin_api/` | REST API for imposter CRUD operations |
| `imposter/` | Imposter lifecycle, request handling, stub matching |
| `predicate/` | Request matching engine (all operators) |
| `behaviors/` | Response modification (wait, copy, lookup, etc.) |
| `proxy/` | HTTP client, forwarding, TLS handling |
| `extensions/` | Rift-specific features (fault injection, flow state, metrics) |
| `scripting/` | Rhai/Lua/JavaScript engine wrappers |
| `config/` | Configuration types and parsing |
| `recording/` | Request capture for proxy mode |

## Building

### Prerequisites

- Rust 1.92+ (check with `rustc --version`)
- Cargo (comes with Rust)

### Commands

```bash
# Debug build (fast compilation)
cargo build

# Release build (optimized)
cargo build --release

# Build specific crate
cargo build -p rift-http-proxy
cargo build -p rift-lint
cargo build -p rift-tui

# Check compilation without building
cargo check
```

### Feature Flags

The `rift-http-proxy` crate has optional features:

```bash
# Build with all features (default)
cargo build -p rift-http-proxy

# Build without Lua scripting
cargo build -p rift-http-proxy --no-default-features --features "redis,javascript"
```

Default features: `redis`, `lua`, `javascript`

## Testing

```bash
# Run all tests
cargo test --all

# Run tests for specific crate
cargo test -p rift-http-proxy
cargo test -p rift-lint

# Run with logging output
RUST_LOG=debug cargo test -- --nocapture

# Run ignored (integration) tests
cargo test --all -- --ignored

# Run specific test
cargo test test_predicate_equals
```

### Benchmarks

```bash
# Run internal benchmarks
cargo bench -p rift-http-proxy

# Run full performance benchmarks
cd tests/benchmark
DURATION=10s CONNECTIONS=20 ./scripts/run-benchmark.sh
```

## Code Quality

```bash
# Format code
cargo fmt

# Lint with Clippy
cargo clippy --all

# Generate documentation
cargo doc --no-deps --open
```

## Running Locally

```bash
# Start server with default settings (port 2525)
cargo run -p rift-http-proxy

# Start with custom port and config
cargo run -p rift-http-proxy -- --port 3000 --configfile examples/basic-api.json

# Run the linter
cargo run -p rift-lint -- examples/basic-api.json

# Run the TUI
cargo run -p rift-tui
```

### Quick Test

```bash
# Terminal 1: Start server
cargo run -p rift-http-proxy

# Terminal 2: Create an imposter
curl -X POST http://localhost:2525/imposters -H "Content-Type: application/json" -d '{
  "port": 4545,
  "protocol": "http",
  "stubs": [{
    "predicates": [{"equals": {"method": "GET", "path": "/hello"}}],
    "responses": [{"is": {"statusCode": 200, "body": "Hello, World!"}}]
  }]
}'

# Terminal 2: Test the mock
curl http://localhost:4545/hello
# Output: Hello, World!
```

## Contributing Guidelines

1. **Fork and branch** - Create feature branches from `master`
2. **Write tests** - Add tests for new functionality
3. **Format and lint** - Run `cargo fmt` and `cargo clippy` before committing
4. **Small PRs** - Keep changes focused and reviewable
5. **Document** - Update docs for user-facing changes

### Commit Convention

Follow conventional commits:
```
feat: add new predicate operator
fix: handle empty body in deepEquals
docs: update scripting documentation
refactor: simplify predicate matching logic
test: add integration tests for proxy mode
```

## Resources

- [Mountebank Documentation](http://www.mbtest.org/docs/gettingStarted) - API compatibility reference
- [Rift Documentation](./docs/) - Feature documentation
- [Examples](./examples/) - Configuration examples

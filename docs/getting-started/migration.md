---
layout: default
title: Migration from Mountebank
parent: Getting Started
nav_order: 2
---

# Migrating from Mountebank to Rift

Rift is designed as a drop-in replacement for Mountebank. This guide covers the migration process and highlights any differences.

---

## Compatibility Overview

Rift maintains full compatibility with Mountebank's HTTP/HTTPS protocol support:

| Feature | Mountebank | Rift | Notes |
|:--------|:-----------|:-----|:------|
| HTTP Imposters | Yes | Yes | Fully compatible |
| HTTPS Imposters | Yes | Yes | Fully compatible |
| REST API | Yes | Yes | Same endpoints |
| JSON Configuration | Yes | Yes | Same format |
| All Predicates | Yes | Yes | equals, contains, matches, exists, etc. |
| JSONPath | Yes | Yes | Same syntax |
| XPath | Yes | Yes | Same syntax |
| Behaviors | Yes | Yes | wait, decorate, copy, lookup |
| Proxy Mode | Yes | Yes | Record and replay |
| Injection | Yes | Yes | JavaScript functions |
| TCP Protocol | Yes | Planned | Coming soon |
| SMTP Protocol | Yes | Planned | Coming soon |

---

## Migration Steps

### Step 1: Replace the Docker Image

If using Docker, simply change the image:

```yaml
# Before (Mountebank)
services:
  mountebank:
    image: bbyars/mountebank:2.9.2
    ports:
      - "2525:2525"
    command: ["start", "--allowInjection"]

# After (Rift)
services:
  rift:
    image: zainalpour/rift-proxy:latest
    ports:
      - "2525:2525"
    environment:
      - MB_ALLOW_INJECTION=true
```

### Step 2: Use Your Existing Configuration

Rift reads Mountebank configuration files directly:

```bash
# Mountebank
mb start --configfile imposters.json

# Rift
docker run -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest --configfile /imposters.json
```

### Step 3: Update Environment Variables

Map Mountebank CLI options to Rift environment variables:

| Mountebank CLI | Rift Environment Variable |
|:---------------|:--------------------------|
| `--port 2525` | `MB_PORT=2525` |
| `--allowInjection` | `MB_ALLOW_INJECTION=true` |
| `--configfile` | `--configfile` (CLI) |
| `--loglevel debug` | `RUST_LOG=debug` |

### Step 4: Verify Functionality

Run your existing test suite against Rift:

```bash
# Your tests should work without modification
npm test
pytest
go test ./...
```

---

## Feature Differences

### Enhanced Performance

Rift provides significantly better performance:

- **20-250x faster** request processing
- **Consistent latency** regardless of stub count
- **Lower memory usage** due to Rust's efficiency

### Additional Features

Rift includes features not in Mountebank:

1. **Native Metrics**: Built-in Prometheus metrics on `/metrics`
2. **Rhai Scripting**: Lightweight embedded scripting (in addition to JavaScript)
3. **Lua Scripting**: High-performance Lua engine option
4. **Flow State**: Stateful testing with Redis backend support

### Minor Differences

| Area | Mountebank | Rift |
|:-----|:-----------|:-----|
| Logging | Custom format | Structured JSON (configurable) |
| Metrics | Third-party | Built-in Prometheus |
| Admin UI | Built-in web UI | API only (UI planned) |

### Known HTTP Behavior Differences

#### Transfer-Encoding Header on 204 Responses

Rift follows HTTP specification more strictly than Mountebank in some cases:

| Scenario | Mountebank | Rift |
|:---------|:-----------|:-----|
| 204 No Content with Transfer-Encoding | Includes header | Strips header |

**Why this happens:** HTTP/1.1 specification (RFC 7230) states that responses with no message body (such as 204 No Content) should not include Transfer-Encoding or Content-Length headers. Rift's underlying HTTP library (hyper) enforces this correctly.

**Impact:** If your stubs define a 204 response with `Transfer-Encoding: chunked`, Rift will return the 204 status but without the Transfer-Encoding header. This is correct HTTP behavior and should not affect most applications.

**Example:**
```json
{
  "responses": [{
    "is": {
      "statusCode": 204,
      "headers": {
        "Transfer-Encoding": "chunked"
      }
    }
  }]
}
```

- **Mountebank**: Returns 204 with `Transfer-Encoding: chunked` header
- **Rift**: Returns 204 without Transfer-Encoding header (per HTTP spec)

---

## Configuration Examples

### Mountebank CLI to Rift

```bash
# Mountebank
mb start \
  --port 2525 \
  --allowInjection \
  --loglevel warn \
  --configfile imposters.json

# Rift (equivalent)
docker run \
  -e MB_PORT=2525 \
  -e MB_ALLOW_INJECTION=true \
  -e RUST_LOG=warn \
  -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest \
  --configfile /imposters.json
```

### Docker Compose

```yaml
version: '3.8'

services:
  rift:
    image: zainalpour/rift-proxy:latest
    ports:
      - "2525:2525"      # Admin API
      - "4545:4545"      # Imposter port
      - "9090:9090"      # Metrics (Rift addition)
    environment:
      - MB_PORT=2525
      - MB_ALLOW_INJECTION=true
      - RUST_LOG=info
    volumes:
      - ./imposters.json:/imposters.json
    command: ["--configfile", "/imposters.json"]
```

---

## Troubleshooting

### "Injection not allowed"

Enable injection via environment variable:

```bash
docker run -e MB_ALLOW_INJECTION=true zainalpour/rift-proxy:latest
```

### Different Response Format

Rift may format JSON responses differently (but equivalently). If your tests compare exact string output, consider comparing parsed JSON instead.

### Missing TCP/SMTP Protocol

These protocols are planned but not yet implemented. For now, continue using Mountebank for TCP/SMTP mocking.

---

## Getting Help

- [GitHub Issues](https://github.com/EtaCassiopeia/rift/issues) - Report bugs or request features
- [Documentation]({{ site.baseurl }}/) - Full documentation
- [Performance Benchmarks]({{ site.baseurl }}/performance/) - Compare with Mountebank

---
layout: default
title: CLI Reference
parent: Configuration
nav_order: 3
---

# Command Line Reference

Rift provides Mountebank-compatible CLI options for easy migration.

---

## Basic Usage

```bash
# Start the server
rift-http-proxy

# With configuration file
rift-http-proxy --configfile imposters.json

# With custom port
rift-http-proxy --port 3525
```

---

## CLI Options

```bash
rift-http-proxy [OPTIONS]

Options:
      --port <PORT>          Admin API port [default: 2525]
      --host <HOST>          Bind hostname [default: 0.0.0.0]
      --configfile <FILE>    Load imposters from JSON file
      --datadir <DIR>        Directory for persistent imposter storage
      --allow-injection      Enable JavaScript injection in responses
      --local-only           Only accept connections from localhost
      --loglevel <LEVEL>     Log level: debug, info, warn, error
      --metrics-port <PORT>  Prometheus metrics port [default: 9090]
      --ip-whitelist <IPS>   Comma-separated allowed IPs
      --mock                 Run in mock mode
      --debug                Enable debug mode
      --nologfile            Disable log file (stdout only)
      --log <FILE>           Log file path
      --pidfile <FILE>       PID file path
      --origin <ORIGIN>      CORS allowed origin
  -h, --help                 Print help
  -V, --version              Print version
```

### Examples

```bash
# Start with custom port
rift-http-proxy --port 3525

# Load configuration and enable injection
rift-http-proxy --configfile imposters.json --allow-injection

# Debug logging
rift-http-proxy --loglevel debug

# Restrict access
rift-http-proxy --local-only
rift-http-proxy --ip-whitelist "192.168.1.0/24,10.0.0.0/8"

# With persistent data directory
rift-http-proxy --datadir ./mb-data
```

---

## Environment Variables

Environment variables override CLI defaults:

| Variable | Description | Default |
|:---------|:------------|:--------|
| `MB_PORT` | Admin API port | `2525` |
| `MB_HOST` | Bind hostname | `0.0.0.0` |
| `MB_CONFIGFILE` | Imposter config file | |
| `MB_DATADIR` | Persistent storage directory | |
| `MB_ALLOW_INJECTION` | Enable injection (`true`/`false`) | `false` |
| `MB_LOCAL_ONLY` | Localhost only | `false` |
| `MB_LOGLEVEL` | Log level | `info` |
| `RIFT_METRICS_PORT` | Prometheus metrics port | `9090` |
| `RUST_LOG` | Detailed log configuration | `info` |

### Docker Example

```bash
docker run \
  -e MB_PORT=2525 \
  -e MB_ALLOW_INJECTION=true \
  -e RUST_LOG=debug \
  -p 2525:2525 \
  -p 9090:9090 \
  zainalpour/rift-proxy:latest
```

### Docker Compose Example

```yaml
version: '3.8'
services:
  rift:
    image: zainalpour/rift-proxy:latest
    ports:
      - "2525:2525"
      - "4545:4545"
      - "9090:9090"
    environment:
      - MB_PORT=2525
      - MB_ALLOW_INJECTION=true
      - RUST_LOG=info
    volumes:
      - ./imposters.json:/imposters.json
    command: ["--configfile", "/imposters.json"]
```

---

## Logging Configuration

### Log Levels

```bash
# Via CLI
rift-http-proxy --loglevel debug

# Via environment
RUST_LOG=debug rift-http-proxy
```

| Level | Description |
|:------|:------------|
| `error` | Only errors |
| `warn` | Warnings and errors |
| `info` | Standard operation (default) |
| `debug` | Detailed debugging |
| `trace` | Very verbose (development) |

### Module-Specific Logging

```bash
# Debug only rift modules
RUST_LOG=rift=debug rift-http-proxy

# Debug HTTP handling
RUST_LOG=rift::http=debug rift-http-proxy

# Multiple modules
RUST_LOG=rift=info,rift::proxy=debug rift-http-proxy
```

---

## Health Check

Rift provides health endpoints:

```bash
# Admin API health
curl http://localhost:2525/

# Metrics health
curl http://localhost:9090/metrics
```

---

## Signal Handling

| Signal | Action |
|:-------|:-------|
| `SIGTERM` | Graceful shutdown |
| `SIGINT` | Graceful shutdown (Ctrl+C) |

```bash
# Graceful shutdown
kill -TERM $(pidof rift-http-proxy)

# Force kill (not recommended)
kill -9 $(pidof rift-http-proxy)
```

---

## Exit Codes

| Code | Meaning |
|:-----|:--------|
| `0` | Success |
| `1` | General error |
| `2` | Configuration error |
| `3` | Port binding error |

---

## Additional CLI Tools

Rift includes additional CLI tools for working with imposters:

### rift-verify

Test imposters by making requests and verifying responses.

```bash
# Verify all imposters
rift-verify

# Verify specific imposter
rift-verify --port 4545 --show-curl
```

See [Stub Analysis]({{ site.baseurl }}/features/stub-analysis/) for details.

### rift-lint

Validate imposter configuration files before loading.

```bash
# Lint all imposters in directory
rift-lint ./imposters/

# Strict mode for CI/CD
rift-lint ./imposters/ --strict

# JSON output
rift-lint ./imposters/ --output json
```

See [Configuration Linting]({{ site.baseurl }}/features/linting/) for details.

---
layout: default
title: CLI Reference
parent: Configuration
nav_order: 3
---

# Command Line Reference

Rift provides both Mountebank-compatible and native CLI options.

---

## Basic Usage

```bash
# Mountebank-compatible mode (default)
rift-http-proxy

# With configuration file
rift-http-proxy --configfile imposters.json

# Native mode with YAML config
rift-http-proxy config.yaml
```

---

## Mountebank-Compatible Options

These options mirror Mountebank's CLI:

```bash
rift-http-proxy [OPTIONS]

Options:
      --port <PORT>          Admin API port [default: 2525]
      --configfile <FILE>    Load imposters from JSON file
      --allowInjection       Enable JavaScript injection in responses
      --loglevel <LEVEL>     Log level: debug, info, warn, error
      --localOnly            Only accept connections from localhost
      --ipWhitelist <IPS>    Comma-separated allowed IPs
  -h, --help                 Print help
  -V, --version              Print version
```

### Examples

```bash
# Start with custom port
rift-http-proxy --port 3525

# Load configuration and enable injection
rift-http-proxy --configfile imposters.json --allowInjection

# Debug logging
rift-http-proxy --loglevel debug

# Restrict access
rift-http-proxy --localOnly
rift-http-proxy --ipWhitelist "192.168.1.0/24,10.0.0.0/8"
```

---

## Environment Variables

Environment variables override CLI defaults:

| Variable | Description | Default |
|:---------|:------------|:--------|
| `MB_PORT` | Admin API port | `2525` |
| `MB_ALLOW_INJECTION` | Enable injection (`true`/`false`) | `false` |
| `MB_LOCAL_ONLY` | Localhost only | `false` |
| `MB_IP_WHITELIST` | Allowed IPs (comma-separated) | |
| `RUST_LOG` | Log level | `info` |
| `RIFT_METRICS_PORT` | Prometheus metrics port | `9090` |
| `RIFT_METRICS_ENABLED` | Enable metrics | `true` |

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

## Native Mode Options

When using YAML configuration:

```bash
rift-http-proxy [OPTIONS] <CONFIG_FILE>

Arguments:
  <CONFIG_FILE>    YAML configuration file

Options:
      --validate       Validate config and exit
      --dry-run        Print effective config and exit
  -h, --help           Print help
```

### Examples

```bash
# Run with native config
rift-http-proxy config.yaml

# Validate configuration
rift-http-proxy --validate config.yaml

# Show effective configuration
rift-http-proxy --dry-run config.yaml
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
| `SIGHUP` | Reload configuration (planned) |

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

---
layout: default
title: Configuration
nav_order: 4
has_children: true
permalink: /configuration/
---

# Configuration

Rift supports two configuration formats:

1. **Mountebank Format** (JSON) - Compatible with Mountebank, recommended for most users
2. **Native Rift Format** (YAML) - Extended features for advanced chaos engineering

---

## Mountebank Format (Recommended)

Use the standard Mountebank JSON format for creating imposters:

```json
{
  "imposters": [
    {
      "port": 4545,
      "protocol": "http",
      "stubs": [
        {
          "predicates": [{ "equals": { "path": "/api/users" } }],
          "responses": [{ "is": { "statusCode": 200, "body": "[]" } }]
        }
      ]
    }
  ]
}
```

Load at startup:

```bash
docker run -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest --configfile /imposters.json
```

Or create dynamically via API:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d @imposter.json
```

[Full Mountebank Format Reference]({{ site.baseurl }}/configuration/mountebank/)

---

## Native Rift Format

Use YAML for advanced chaos engineering scenarios with extended features:

```yaml
mode: sidecar
listen:
  port: 8080
upstream:
  host: "localhost"
  port: 8081
rules:
  - id: "latency-injection"
    match:
      methods: ["GET"]
      path:
        prefix: "/api"
    fault:
      latency:
        probability: 0.3
        min_ms: 100
        max_ms: 500
```

Run with native config:

```bash
./rift-http-proxy config.yaml
```

[Full Native Format Reference]({{ site.baseurl }}/configuration/native/)

---

## Environment Variables

Configure Rift behavior via environment variables:

| Variable | Description | Default |
|:---------|:------------|:--------|
| `MB_PORT` | Admin API port (Mountebank mode) | `2525` |
| `MB_ALLOW_INJECTION` | Enable JavaScript injection | `false` |
| `RUST_LOG` | Log level (trace, debug, info, warn, error) | `info` |
| `RIFT_METRICS_PORT` | Prometheus metrics port | `9090` |

```bash
docker run -e MB_PORT=2525 -e MB_ALLOW_INJECTION=true \
  -e RUST_LOG=debug zainalpour/rift-proxy:latest
```

---

## Command Line Options

```bash
rift-http-proxy [OPTIONS] [CONFIG_FILE]

Arguments:
  [CONFIG_FILE]  Path to configuration file (YAML for native, JSON for Mountebank)

Options:
      --configfile <FILE>  Mountebank-compatible config file
      --port <PORT>        Admin API port (default: 2525)
      --allowInjection     Enable JavaScript injection
  -h, --help               Print help
  -V, --version            Print version
```

---

## When to Use Each Format

### Use Mountebank Format When:
- Migrating from Mountebank
- Creating API mocks for testing
- Working with existing Mountebank tooling
- Need service virtualization

### Use Native Rift Format When:
- Running chaos engineering experiments
- Need advanced fault injection (probabilistic, scripted)
- Deploying as a sidecar in Kubernetes
- Need per-upstream routing rules

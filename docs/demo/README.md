# Rift Demo

Quick-start demos for Rift in different modes.

> **Note**: These demos use a locally-built Docker image. Run `docker build -t rift-proxy:local -f crates/rift-http-proxy/Dockerfile .` from the project root first.

## Demo 1: Mountebank Mode (HTTP)

The primary way to use Rift - Mountebank-compatible mock server.

### Start

```bash
docker compose up -d
```

### Test

```bash
# Health check
curl http://localhost:4545/health

# List users
curl http://localhost:4545/api/users

# Get single user
curl http://localhost:4545/api/users/1

# Create user
curl -X POST http://localhost:4545/api/users \
  -H "Content-Type: application/json" \
  -d '{"name": "Charlie"}'

# Test slow endpoint (2s delay)
time curl http://localhost:4545/api/slow

# Test error endpoint
curl http://localhost:4545/api/error

# Order API (different port)
curl http://localhost:4546/api/orders
```

### Manage Imposters

```bash
# List imposters
curl http://localhost:2525/imposters

# Get imposter details
curl http://localhost:2525/imposters/4545

# View recorded requests
curl http://localhost:2525/imposters/4545 | jq '.requests'

# Add new stub dynamically
curl -X POST http://localhost:2525/imposters/4545/stubs \
  -H "Content-Type: application/json" \
  -d '{
    "stub": {
      "predicates": [{ "equals": { "path": "/api/new" } }],
      "responses": [{ "is": { "statusCode": 200, "body": "New endpoint" } }]
    }
  }'

# Delete imposter
curl -X DELETE http://localhost:2525/imposters/4545
```

### Cleanup

```bash
docker compose down
```

---

## Demo 2: HTTPS/TLS Mode

Demonstrates Rift's TLS support with custom certificates.

### Prerequisites

Generate self-signed certificates:

```bash
./generate-certs.sh
```

### Start

```bash
docker compose -f docker-compose-https.yml up -d
```

### Test

```bash
# Basic HTTPS request (with CA certificate)
curl --cacert certs/ca.crt https://localhost:8443/get

# Or skip verification (development only)
curl -k https://localhost:8443/get

# Test POST with fault injection
curl --cacert certs/ca.crt -X POST https://localhost:8443/post \
  -d '{"test": "data"}'

# TLS test endpoint
curl --cacert certs/ca.crt https://localhost:8443/anything/tls-test

# TLS test with slow mode
time curl --cacert certs/ca.crt https://localhost:8443/anything/tls-test \
  -H "X-TLS-Test: slow"

# View metrics (HTTP)
curl http://localhost:9091/metrics | grep rift
```

### Trust the CA (Optional)

To avoid using `--cacert` or `-k`:

**macOS:**
```bash
sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain certs/ca.crt
```

**Linux:**
```bash
sudo cp certs/ca.crt /usr/local/share/ca-certificates/rift-demo.crt
sudo update-ca-certificates
```

### Cleanup

```bash
docker compose -f docker-compose-https.yml down
rm -rf certs/  # Optional: remove generated certificates
```

---

## Configuration Files

| File | Description |
|:-----|:------------|
| `imposters.json` | Mountebank HTTP imposter config |
| `docker-compose.yml` | HTTP demo |
| `docker-compose-https.yml` | HTTPS/TLS demo |
| `generate-certs.sh` | Certificate generation script |

---

## Rift Extensions (`_rift` namespace)

Rift extends Mountebank with advanced features through the `_rift` namespace:

- **Flow State**: Stateful testing with in-memory or Redis backends
- **Fault Injection**: Probabilistic latency, error, and TCP faults
- **Scripting**: Multi-engine scripting (Rhai, Lua, JavaScript)

Example imposter with `_rift` extensions:

```json
{
  "port": 4545,
  "protocol": "http",
  "_rift": {
    "flowState": {"backend": "inmemory", "ttlSeconds": 300}
  },
  "stubs": [{
    "predicates": [{"equals": {"path": "/api/test"}}],
    "responses": [{
      "is": {"statusCode": 200, "body": "OK"},
      "_rift": {
        "fault": {
          "latency": {"probability": 0.3, "minMs": 100, "maxMs": 500}
        }
      }
    }]
  }]
}
```

See the [Rift Extensions documentation](/docs/features/rift-extensions.md) for more details.

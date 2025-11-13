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

## Demo 3: Native Mode (Advanced Features)

Demonstrates features not available in Mountebank format:
- Probabilistic fault injection
- Flow state for stateful scenarios
- Rhai scripting

### Start

```bash
docker compose -f docker-compose-native.yml up -d
```

### Test Probabilistic Faults

```bash
# Basic proxy request
curl http://localhost:8080/get

# Latency injection (30% probability on /delay/*)
echo "Testing latency injection..."
for i in {1..5}; do
  START=$(date +%s%3N)
  curl -s http://localhost:8080/delay/1 -o /dev/null
  END=$(date +%s%3N)
  echo "Request $i: $((END - START))ms"
done

# Error injection (10% probability on POST)
echo "Testing error injection..."
for i in {1..10}; do
  STATUS=$(curl -s -X POST http://localhost:8080/post \
    -d "test" -w "%{http_code}" -o /dev/null)
  echo "Request $i: HTTP $STATUS"
done
```

### Test Conditional Faults

```bash
# Normal request
curl http://localhost:8080/anything/conditional \
  -X POST -d "test"

# Slow mode (2s delay)
time curl http://localhost:8080/anything/conditional \
  -X POST -H "X-Test-Mode: slow" -d "test"

# Fail mode
curl http://localhost:8080/anything/conditional \
  -X POST -H "X-Test-Mode: fail" -d "test"
```

### View Metrics

```bash
curl http://localhost:9090/metrics | grep rift
```

### Cleanup

```bash
docker compose -f docker-compose-native.yml down
```

---

## Configuration Files

| File | Description |
|:-----|:------------|
| `imposters.json` | Mountebank HTTP imposter config |
| `native-config.yaml` | Native Rift config with advanced features |
| `native-config-https.yaml` | Native Rift config with TLS |
| `docker-compose.yml` | HTTP demo (Mountebank mode) |
| `docker-compose-https.yml` | HTTPS/TLS demo |
| `docker-compose-native.yml` | Native mode demo |
| `generate-certs.sh` | Certificate generation script |

---

## When to Use Each Mode

**Mountebank Mode** (recommended for most users):
- Compatible with existing Mountebank configurations
- Standard API mocking for integration tests
- Simple configuration with JSON format

**HTTPS/TLS Mode** (secure testing):
- Testing with TLS-enabled endpoints
- Simulating production HTTPS environments
- Testing client certificate handling
- Verifying TLS configuration

**Native Mode** (advanced use cases):
- Chaos engineering with probabilistic faults
- Stateful testing with flow state
- Complex conditional logic with scripting
- Custom fault injection scenarios

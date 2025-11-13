---
layout: default
title: Imposters
parent: Mountebank Compatibility
nav_order: 1
---

# Imposters

An imposter is a mock server that listens on a specific port and responds to requests based on configured stubs.

---

## Creating an Imposter

### Basic HTTP Imposter

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "name": "My Service Mock",
    "stubs": [{
      "responses": [{
        "is": { "statusCode": 200, "body": "Hello" }
      }]
    }]
  }'
```

### HTTPS Imposter

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4546,
    "protocol": "https",
    "name": "Secure Service Mock",
    "key": "-----BEGIN RSA PRIVATE KEY-----\n...\n-----END RSA PRIVATE KEY-----",
    "cert": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "stubs": [{
      "responses": [{
        "is": { "statusCode": 200, "body": "Secure Hello" }
      }]
    }]
  }'
```

---

## Imposter Configuration

| Field | Type | Required | Description |
|:------|:-----|:---------|:------------|
| `port` | number | Yes | Port to listen on |
| `protocol` | string | Yes | `http` or `https` |
| `name` | string | No | Human-readable name |
| `stubs` | array | No | Request/response mappings |
| `defaultResponse` | object | No | Response when no stub matches |
| `recordRequests` | boolean | No | Store requests for verification |
| `key` | string | HTTPS only | PEM-encoded private key |
| `cert` | string | HTTPS only | PEM-encoded certificate |
| `mutualAuth` | boolean | No | Require client certificate |

---

## Stubs

Each stub contains predicates (matching rules) and responses:

```json
{
  "stubs": [
    {
      "predicates": [
        { "equals": { "method": "GET", "path": "/api/users" } }
      ],
      "responses": [
        { "is": { "statusCode": 200, "body": "[]" } }
      ]
    }
  ]
}
```

### Multiple Responses (Round-Robin)

When a stub has multiple responses, they cycle through:

```json
{
  "stubs": [{
    "predicates": [{ "equals": { "path": "/flip" } }],
    "responses": [
      { "is": { "body": "heads" } },
      { "is": { "body": "tails" } }
    ]
  }]
}
```

First request returns "heads", second returns "tails", third returns "heads", etc.

---

## Default Response

Configure a fallback response when no stub matches:

```json
{
  "port": 4545,
  "protocol": "http",
  "defaultResponse": {
    "statusCode": 404,
    "headers": { "Content-Type": "application/json" },
    "body": { "error": "Not Found" }
  },
  "stubs": [...]
}
```

---

## Recording Requests

Enable request recording for verification in tests:

```json
{
  "port": 4545,
  "protocol": "http",
  "recordRequests": true,
  "stubs": [...]
}
```

Retrieve recorded requests:

```bash
curl http://localhost:2525/imposters/4545

# Response includes:
{
  "requests": [
    {
      "method": "GET",
      "path": "/api/users",
      "headers": {...},
      "body": "",
      "timestamp": "2024-01-15T10:30:00.000Z"
    }
  ]
}
```

---

## Managing Imposters

### List All Imposters

```bash
curl http://localhost:2525/imposters

# Response:
{
  "imposters": [
    { "port": 4545, "protocol": "http", "name": "User Service" },
    { "port": 4546, "protocol": "https", "name": "Payment Service" }
  ]
}
```

### Get Imposter Details

```bash
curl http://localhost:2525/imposters/4545

# Response includes full configuration and recorded requests
```

### Delete Single Imposter

```bash
curl -X DELETE http://localhost:2525/imposters/4545
```

### Delete All Imposters

```bash
curl -X DELETE http://localhost:2525/imposters
```

---

## Loading from Configuration File

### JSON Format

Create `imposters.json`:

```json
{
  "imposters": [
    {
      "port": 4545,
      "protocol": "http",
      "stubs": [...]
    },
    {
      "port": 4546,
      "protocol": "http",
      "stubs": [...]
    }
  ]
}
```

Load on startup:

```bash
docker run -v $(pwd)/imposters.json:/imposters.json \
  zainalpour/rift-proxy:latest --configfile /imposters.json
```

### EJS Templates

Use EJS for dynamic configuration:

```json
{
  "imposters": [
    {
      "port": "<%= port || 4545 %>",
      "protocol": "http",
      "stubs": [...]
    }
  ]
}
```

---

## Best Practices

1. **Use meaningful names** - Makes debugging easier
2. **Order stubs specifically** - More specific predicates first
3. **Enable recording in tests** - Verify expected requests
4. **Use default responses** - Clear error messages for unmatched requests
5. **Separate imposters by service** - One imposter per external dependency

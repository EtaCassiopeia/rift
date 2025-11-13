---
layout: default
title: API Reference
nav_order: 7
permalink: /api/
---

# REST API Reference

Rift provides a Mountebank-compatible REST API for managing imposters.

---

## Base URL

```
http://localhost:2525
```

---

## Root

### GET /

Get API information and links.

**Response:**
```json
{
  "_links": {
    "imposters": { "href": "/imposters" },
    "config": { "href": "/config" },
    "logs": { "href": "/logs" }
  }
}
```

---

## Imposters

### GET /imposters

List all imposters.

**Query Parameters:**
- `replayable` (boolean) - Include full stub details for export

**Response:**
```json
{
  "imposters": [
    {
      "port": 4545,
      "protocol": "http",
      "name": "User Service",
      "numberOfRequests": 42
    },
    {
      "port": 4546,
      "protocol": "https",
      "name": "Payment Service",
      "numberOfRequests": 15
    }
  ]
}
```

**Example:**
```bash
curl http://localhost:2525/imposters
curl "http://localhost:2525/imposters?replayable=true"
```

---

### POST /imposters

Create a new imposter.

**Request Body:**
```json
{
  "port": 4545,
  "protocol": "http",
  "name": "My Service",
  "stubs": [
    {
      "predicates": [{ "equals": { "path": "/test" } }],
      "responses": [{ "is": { "statusCode": 200, "body": "OK" } }]
    }
  ]
}
```

**Response:** `201 Created`
```json
{
  "port": 4545,
  "protocol": "http",
  "name": "My Service",
  "numberOfRequests": 0,
  "stubs": [...]
}
```

**Example:**
```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "stubs": [{
      "responses": [{ "is": { "statusCode": 200 } }]
    }]
  }'
```

---

### PUT /imposters

Replace all imposters (bulk create/update).

**Request Body:**
```json
{
  "imposters": [
    { "port": 4545, "protocol": "http", "stubs": [...] },
    { "port": 4546, "protocol": "http", "stubs": [...] }
  ]
}
```

**Response:** `200 OK`
```json
{
  "imposters": [...]
}
```

---

### GET /imposters/{port}

Get imposter details.

**Query Parameters:**
- `replayable` (boolean) - Include full configuration for export
- `removeProxies` (boolean) - Exclude proxy stubs

**Response:**
```json
{
  "port": 4545,
  "protocol": "http",
  "name": "My Service",
  "numberOfRequests": 42,
  "requests": [
    {
      "method": "GET",
      "path": "/test",
      "headers": {...},
      "timestamp": "2024-01-15T10:30:00.000Z"
    }
  ],
  "stubs": [...]
}
```

**Example:**
```bash
curl http://localhost:2525/imposters/4545
curl "http://localhost:2525/imposters/4545?replayable=true"
```

---

### DELETE /imposters/{port}

Delete an imposter.

**Query Parameters:**
- `replayable` (boolean) - Return imposter config before deletion

**Response:** `200 OK`
```json
{
  "port": 4545,
  "protocol": "http",
  "stubs": [...]
}
```

**Example:**
```bash
curl -X DELETE http://localhost:2525/imposters/4545
```

---

### DELETE /imposters

Delete all imposters.

**Response:** `200 OK`
```json
{
  "imposters": [...]
}
```

**Example:**
```bash
curl -X DELETE http://localhost:2525/imposters
```

---

## Stub Management

### POST /imposters/{port}/stubs

Add a stub to an existing imposter.

**Request Body:**
```json
{
  "stub": {
    "predicates": [{ "equals": { "path": "/new" } }],
    "responses": [{ "is": { "statusCode": 200 } }]
  },
  "index": 0
}
```

**Response:** `200 OK`

**Example:**
```bash
curl -X POST http://localhost:2525/imposters/4545/stubs \
  -H "Content-Type: application/json" \
  -d '{
    "stub": {
      "predicates": [{ "equals": { "path": "/new" } }],
      "responses": [{ "is": { "statusCode": 201 } }]
    }
  }'
```

---

### PUT /imposters/{port}/stubs/{index}

Replace a stub at a specific index.

**Request Body:**
```json
{
  "predicates": [{ "equals": { "path": "/updated" } }],
  "responses": [{ "is": { "statusCode": 200 } }]
}
```

---

### DELETE /imposters/{port}/stubs/{index}

Delete a stub at a specific index.

---

## Requests

### GET /imposters/{port}/requests

Get recorded requests (if `recordRequests: true`).

**Response:**
```json
{
  "requests": [
    {
      "method": "GET",
      "path": "/api/users",
      "query": {},
      "headers": {
        "host": "localhost:4545",
        "user-agent": "curl/7.88.0"
      },
      "body": "",
      "timestamp": "2024-01-15T10:30:00.000Z"
    }
  ]
}
```

---

### DELETE /imposters/{port}/requests

Clear recorded requests.

---

### DELETE /imposters/{port}/savedRequests

Clear saved proxy requests.

---

## Configuration

### GET /config

Get current configuration.

**Response:**
```json
{
  "options": {
    "port": 2525,
    "allowInjection": true,
    "localOnly": false
  }
}
```

---

## Logs

### GET /logs

Get server logs (if logging enabled).

**Query Parameters:**
- `startIndex` (number) - Start from this log entry
- `endIndex` (number) - End at this log entry

---

## Error Responses

### 400 Bad Request

Invalid request body or parameters.

```json
{
  "errors": [
    {
      "code": "bad data",
      "message": "invalid JSON"
    }
  ]
}
```

### 404 Not Found

Imposter doesn't exist.

```json
{
  "errors": [
    {
      "code": "no such resource",
      "message": "Imposter not found on port 4545"
    }
  ]
}
```

### 409 Conflict

Port already in use.

```json
{
  "errors": [
    {
      "code": "port conflict",
      "message": "Port 4545 is already in use"
    }
  ]
}
```

---

## Common Patterns

### Export and Reimport

```bash
# Export
curl "http://localhost:2525/imposters?replayable=true" > imposters.json

# Clear
curl -X DELETE http://localhost:2525/imposters

# Reimport
curl -X PUT http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d @imposters.json
```

### Verify Requests

```bash
# Create imposter with recording
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "recordRequests": true,
    "stubs": [...]
  }'

# Run tests...

# Verify requests
curl http://localhost:2525/imposters/4545 | jq '.requests'
```

---
layout: default
title: Quick Start
parent: Getting Started
nav_order: 1
---

# Quick Start Tutorial

This tutorial walks you through creating various types of imposters with Rift.

---

## Prerequisites

Ensure Rift is running:

```bash
docker run -p 2525:2525 ghcr.io/etacassiopeia/rift-proxy:latest
```

---

## Basic REST API Mock

Create a mock for a simple user API:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4545,
    "protocol": "http",
    "name": "User API",
    "stubs": [
      {
        "predicates": [{ "equals": { "method": "GET", "path": "/users" } }],
        "responses": [{
          "is": {
            "statusCode": 200,
            "headers": { "Content-Type": "application/json" },
            "body": [
              { "id": 1, "name": "Alice" },
              { "id": 2, "name": "Bob" }
            ]
          }
        }]
      },
      {
        "predicates": [{ "equals": { "method": "GET", "path": "/users/1" } }],
        "responses": [{
          "is": {
            "statusCode": 200,
            "headers": { "Content-Type": "application/json" },
            "body": { "id": 1, "name": "Alice", "email": "alice@example.com" }
          }
        }]
      },
      {
        "predicates": [{ "equals": { "method": "POST", "path": "/users" } }],
        "responses": [{
          "is": {
            "statusCode": 201,
            "headers": { "Content-Type": "application/json" },
            "body": { "id": 3, "name": "New User" }
          }
        }]
      }
    ]
  }'
```

Test the endpoints:

```bash
# List users
curl http://localhost:4545/users
# [{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}]

# Get user by ID
curl http://localhost:4545/users/1
# {"id":1,"name":"Alice","email":"alice@example.com"}

# Create user
curl -X POST http://localhost:4545/users -d '{"name":"Charlie"}'
# {"id":3,"name":"New User"}
```

---

## Pattern Matching with Regex

Match dynamic paths using regex:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4546,
    "protocol": "http",
    "stubs": [{
      "predicates": [{
        "matches": {
          "path": "/users/\\d+"
        }
      }],
      "responses": [{
        "is": {
          "statusCode": 200,
          "body": { "message": "User found" }
        }
      }]
    }]
  }'
```

```bash
curl http://localhost:4546/users/123    # User found
curl http://localhost:4546/users/999    # User found
curl http://localhost:4546/users/abc    # No match (404)
```

---

## JSON Body Matching

Match requests based on JSON body content:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4547,
    "protocol": "http",
    "stubs": [
      {
        "predicates": [{
          "equals": {
            "method": "POST",
            "body": { "action": "login", "username": "admin" }
          }
        }],
        "responses": [{
          "is": {
            "statusCode": 200,
            "body": { "token": "abc123", "role": "admin" }
          }
        }]
      },
      {
        "predicates": [{
          "contains": {
            "body": { "action": "login" }
          }
        }],
        "responses": [{
          "is": {
            "statusCode": 200,
            "body": { "token": "xyz789", "role": "user" }
          }
        }]
      }
    ]
  }'
```

```bash
# Admin login (exact match)
curl -X POST http://localhost:4547/login \
  -H "Content-Type: application/json" \
  -d '{"action":"login","username":"admin"}'
# {"token":"abc123","role":"admin"}

# Regular user login (contains match)
curl -X POST http://localhost:4547/login \
  -H "Content-Type: application/json" \
  -d '{"action":"login","username":"bob"}'
# {"token":"xyz789","role":"user"}
```

---

## JSONPath Predicates

Match specific values in JSON using JSONPath:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4548,
    "protocol": "http",
    "stubs": [{
      "predicates": [{
        "jsonpath": {
          "selector": "$.order.total",
          "equals": 100
        }
      }],
      "responses": [{
        "is": {
          "statusCode": 200,
          "body": { "status": "approved", "discount": "10%" }
        }
      }]
    }]
  }'
```

```bash
curl -X POST http://localhost:4548/checkout \
  -H "Content-Type: application/json" \
  -d '{"order":{"items":["a","b"],"total":100}}'
# {"status":"approved","discount":"10%"}
```

---

## Simulating Delays

Add latency to responses for testing timeouts:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4549,
    "protocol": "http",
    "stubs": [{
      "predicates": [{ "equals": { "path": "/slow" } }],
      "responses": [{
        "is": {
          "statusCode": 200,
          "body": "Response after delay"
        },
        "_behaviors": {
          "wait": 2000
        }
      }]
    }]
  }'
```

```bash
time curl http://localhost:4549/slow
# Response after delay
# real 0m2.015s
```

---

## Error Simulation

Test error handling in your application:

```bash
curl -X POST http://localhost:2525/imposters \
  -H "Content-Type: application/json" \
  -d '{
    "port": 4550,
    "protocol": "http",
    "stubs": [
      {
        "predicates": [{ "equals": { "path": "/error/400" } }],
        "responses": [{
          "is": {
            "statusCode": 400,
            "body": { "error": "Bad Request", "message": "Invalid input" }
          }
        }]
      },
      {
        "predicates": [{ "equals": { "path": "/error/500" } }],
        "responses": [{
          "is": {
            "statusCode": 500,
            "body": { "error": "Internal Server Error" }
          }
        }]
      },
      {
        "predicates": [{ "equals": { "path": "/error/503" } }],
        "responses": [{
          "is": {
            "statusCode": 503,
            "headers": { "Retry-After": "60" },
            "body": { "error": "Service Unavailable" }
          }
        }]
      }
    ]
  }'
```

---

## Managing Imposters

### List All Imposters

```bash
curl http://localhost:2525/imposters
```

### Get Imposter Details

```bash
curl http://localhost:2525/imposters/4545
```

### Delete an Imposter

```bash
curl -X DELETE http://localhost:2525/imposters/4545
```

### Delete All Imposters

```bash
curl -X DELETE http://localhost:2525/imposters
```

---

## Next Steps

- [Predicates Reference]({{ site.baseurl }}/mountebank/predicates/) - All predicate types
- [Behaviors Guide]({{ site.baseurl }}/mountebank/behaviors/) - wait, decorate, copy
- [Proxy Mode]({{ site.baseurl }}/mountebank/proxy/) - Record and replay

---
layout: default
title: Responses
parent: Mountebank Compatibility
nav_order: 3
---

# Responses

Responses define what an imposter returns when a stub's predicates match.

---

## Response Types

### is (Static Response)

Return a fixed response:

```json
{
  "is": {
    "statusCode": 200,
    "headers": {
      "Content-Type": "application/json",
      "X-Custom-Header": "value"
    },
    "body": {
      "message": "Success",
      "data": { "id": 1 }
    }
  }
}
```

### proxy (Forward Request)

Forward requests to a real server and optionally record responses:

```json
{
  "proxy": {
    "to": "https://api.example.com",
    "mode": "proxyAlways",
    "predicateGenerators": [{
      "matches": { "path": true, "method": true }
    }]
  }
}
```

### inject (Dynamic Response)

Generate responses with JavaScript:

```json
{
  "inject": "function(request, state, logger) { return { statusCode: 200, body: 'Request path: ' + request.path }; }"
}
```

---

## Static Responses (is)

### Status Codes

```json
{ "is": { "statusCode": 201 } }
{ "is": { "statusCode": 400 } }
{ "is": { "statusCode": 500 } }
```

### Headers

```json
{
  "is": {
    "statusCode": 200,
    "headers": {
      "Content-Type": "application/json",
      "Cache-Control": "no-cache",
      "X-Request-Id": "abc123"
    }
  }
}
```

### Body Types

**String body:**
```json
{ "is": { "body": "Hello, World!" } }
```

**JSON body (auto-serialized):**
```json
{
  "is": {
    "body": {
      "users": [
        { "id": 1, "name": "Alice" },
        { "id": 2, "name": "Bob" }
      ]
    }
  }
}
```

**XML body:**
```json
{
  "is": {
    "headers": { "Content-Type": "application/xml" },
    "body": "<?xml version=\"1.0\"?><user><id>1</id></user>"
  }
}
```

**Binary body (base64):**
```json
{
  "is": {
    "body": "SGVsbG8gV29ybGQ=",
    "_mode": "binary"
  }
}
```

---

## Multiple Responses

Stubs can have multiple responses that cycle through (round-robin):

```json
{
  "stubs": [{
    "predicates": [{ "equals": { "path": "/random" } }],
    "responses": [
      { "is": { "body": "Response 1" } },
      { "is": { "body": "Response 2" } },
      { "is": { "body": "Response 3" } }
    ]
  }]
}
```

---

## Proxy Responses

Forward requests to real servers and optionally record for later playback.

### Proxy Modes

**proxyAlways** - Always forward, record each response:
```json
{
  "proxy": {
    "to": "https://api.example.com",
    "mode": "proxyAlways"
  }
}
```

**proxyOnce** - Forward first request, replay recorded response:
```json
{
  "proxy": {
    "to": "https://api.example.com",
    "mode": "proxyOnce"
  }
}
```

**proxyTransparent** - Forward without recording:
```json
{
  "proxy": {
    "to": "https://api.example.com",
    "mode": "proxyTransparent"
  }
}
```

### Predicate Generators

Control how recorded stubs are created:

```json
{
  "proxy": {
    "to": "https://api.example.com",
    "predicateGenerators": [{
      "matches": {
        "path": true,
        "method": true,
        "query": true
      }
    }]
  }
}
```

### Adding Behaviors to Proxied Responses

```json
{
  "proxy": {
    "to": "https://api.example.com",
    "addDecorateBehavior": "function(request, response) { response.headers['X-Proxied'] = 'true'; return response; }"
  }
}
```

---

## Injection Responses

Generate dynamic responses using JavaScript:

```json
{
  "inject": "function(request, state, logger) { \
    var userId = request.path.split('/')[2]; \
    return { \
      statusCode: 200, \
      headers: { 'Content-Type': 'application/json' }, \
      body: JSON.stringify({ id: userId, name: 'User ' + userId }) \
    }; \
  }"
}
```

### Request Object

Available properties in injection function:

```javascript
request.method    // "GET", "POST", etc.
request.path      // "/api/users/123"
request.query     // { page: "1" }
request.headers   // { "content-type": "application/json" }
request.body      // Request body (string or parsed JSON)
```

### State Object

Persist data across requests:

```javascript
function(request, state, logger) {
  // Initialize counter
  state.counter = state.counter || 0;
  state.counter++;

  return {
    statusCode: 200,
    body: { count: state.counter }
  };
}
```

### Logger Object

Write to Rift logs:

```javascript
function(request, state, logger) {
  logger.info("Processing request to " + request.path);
  return { statusCode: 200 };
}
```

---

## Response Templates

Use EJS templates for dynamic content:

```json
{
  "is": {
    "statusCode": 200,
    "headers": { "Content-Type": "application/json" },
    "body": "{ \"path\": \"<%- request.path %>\", \"timestamp\": \"<%- new Date().toISOString() %>\" }"
  },
  "_behaviors": {
    "decorate": "function(request, response) { return response; }"
  }
}
```

---

## Error Responses

### Client Errors (4xx)

```json
{
  "is": {
    "statusCode": 400,
    "body": { "error": "Bad Request", "message": "Invalid input" }
  }
}

{
  "is": {
    "statusCode": 401,
    "headers": { "WWW-Authenticate": "Bearer" },
    "body": { "error": "Unauthorized" }
  }
}

{
  "is": {
    "statusCode": 404,
    "body": { "error": "Not Found" }
  }
}
```

### Server Errors (5xx)

```json
{
  "is": {
    "statusCode": 500,
    "body": { "error": "Internal Server Error" }
  }
}

{
  "is": {
    "statusCode": 503,
    "headers": { "Retry-After": "60" },
    "body": { "error": "Service Unavailable" }
  }
}
```

---

## Best Practices

1. **Set Content-Type** - Always include appropriate Content-Type header
2. **Use JSON for APIs** - Return `body` as object for automatic serialization
3. **Include error details** - Meaningful error responses help debugging
4. **Use proxy for recording** - Record real API responses for reliable mocks
5. **Keep injection simple** - Complex logic is harder to maintain

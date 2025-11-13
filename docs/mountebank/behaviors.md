---
layout: default
title: Behaviors
parent: Mountebank Compatibility
nav_order: 4
---

# Behaviors

Behaviors modify responses before they are sent to the client. They enable latency simulation, response transformation, and dynamic content.

---

## Adding Behaviors

Behaviors are added to responses using `_behaviors`:

```json
{
  "is": {
    "statusCode": 200,
    "body": "Hello"
  },
  "_behaviors": {
    "wait": 1000,
    "decorate": "function(request, response) { response.body += ' World'; return response; }"
  }
}
```

---

## wait

Add latency to responses. Essential for testing timeout handling.

### Fixed Delay

```json
{
  "_behaviors": {
    "wait": 2000
  }
}
```

Adds exactly 2000ms delay.

### Random Delay

```json
{
  "_behaviors": {
    "wait": {
      "inject": "function() { return Math.floor(Math.random() * 1000) + 500; }"
    }
  }
}
```

Returns random delay between 500-1500ms.

### Use Cases

**Test client timeouts:**
```json
{
  "stubs": [{
    "predicates": [{ "equals": { "path": "/slow-endpoint" } }],
    "responses": [{
      "is": { "statusCode": 200 },
      "_behaviors": { "wait": 5000 }
    }]
  }]
}
```

**Simulate network latency:**
```json
{
  "_behaviors": {
    "wait": {
      "inject": "function() { return Math.floor(Math.random() * 100) + 50; }"
    }
  }
}
```

---

## decorate

Transform responses using JavaScript. The function receives request and response, and must return the modified response.

### Basic Transformation

```json
{
  "is": {
    "statusCode": 200,
    "body": { "data": [] }
  },
  "_behaviors": {
    "decorate": "function(request, response) { response.body.timestamp = Date.now(); return response; }"
  }
}
```

### Add Request Info to Response

```json
{
  "_behaviors": {
    "decorate": "function(request, response) { \
      response.headers = response.headers || {}; \
      response.headers['X-Request-Path'] = request.path; \
      response.headers['X-Request-Method'] = request.method; \
      return response; \
    }"
  }
}
```

### Conditional Modification

```json
{
  "_behaviors": {
    "decorate": "function(request, response) { \
      if (request.headers['X-Debug'] === 'true') { \
        response.body = { \
          original: response.body, \
          debug: { path: request.path, query: request.query } \
        }; \
      } \
      return response; \
    }"
  }
}
```

### Parse and Modify JSON

```json
{
  "_behaviors": {
    "decorate": "function(request, response) { \
      var body = typeof response.body === 'string' ? JSON.parse(response.body) : response.body; \
      body.serverTime = new Date().toISOString(); \
      response.body = body; \
      return response; \
    }"
  }
}
```

---

## copy

Copy values from the request to the response. Useful for echoing request data.

### Copy from Path

```json
{
  "is": {
    "statusCode": 200,
    "body": { "id": "${id}" }
  },
  "_behaviors": {
    "copy": {
      "from": { "path": "/users/(\\d+)" },
      "into": "${id}",
      "using": { "method": "regex", "selector": "$1" }
    }
  }
}
```

Request to `/users/123` returns `{ "id": "123" }`.

### Copy from Query

```json
{
  "is": {
    "statusCode": 200,
    "body": "Page: ${page}"
  },
  "_behaviors": {
    "copy": {
      "from": "query",
      "into": "${page}",
      "using": { "method": "jsonpath", "selector": "$.page" }
    }
  }
}
```

### Copy from Headers

```json
{
  "is": {
    "statusCode": 200,
    "headers": { "X-Request-Id": "${reqId}" }
  },
  "_behaviors": {
    "copy": {
      "from": "headers",
      "into": "${reqId}",
      "using": { "method": "jsonpath", "selector": "$['X-Request-Id']" }
    }
  }
}
```

### Copy from Body

```json
{
  "is": {
    "statusCode": 200,
    "body": { "received": "${name}" }
  },
  "_behaviors": {
    "copy": {
      "from": "body",
      "into": "${name}",
      "using": { "method": "jsonpath", "selector": "$.user.name" }
    }
  }
}
```

### Multiple Copies

```json
{
  "_behaviors": {
    "copy": [
      {
        "from": { "path": "/orders/(\\d+)" },
        "into": "${orderId}",
        "using": { "method": "regex", "selector": "$1" }
      },
      {
        "from": "query",
        "into": "${format}",
        "using": { "method": "jsonpath", "selector": "$.format" }
      }
    ]
  }
}
```

---

## lookup

Look up data from external sources (CSV files, etc.).

### CSV Lookup

```json
{
  "is": {
    "statusCode": 200,
    "body": { "name": "${name}", "email": "${email}" }
  },
  "_behaviors": {
    "lookup": {
      "key": {
        "from": { "path": "/users/(\\d+)" },
        "using": { "method": "regex", "selector": "$1" }
      },
      "fromDataSource": {
        "csv": {
          "path": "users.csv",
          "keyColumn": "id"
        }
      },
      "into": "${row}"
    }
  }
}
```

With `users.csv`:
```csv
id,name,email
1,Alice,alice@example.com
2,Bob,bob@example.com
```

Request to `/users/1` returns `{ "name": "Alice", "email": "alice@example.com" }`.

---

## Behavior Order

When multiple behaviors are defined, they execute in this order:

1. **copy** - Copy request values into response
2. **lookup** - Perform data lookups
3. **decorate** - Transform the response
4. **wait** - Add delay before sending

---

## Combining Behaviors

```json
{
  "is": {
    "statusCode": 200,
    "body": { "userId": "${id}", "processed": false }
  },
  "_behaviors": {
    "copy": {
      "from": { "path": "/users/(\\d+)" },
      "into": "${id}",
      "using": { "method": "regex", "selector": "$1" }
    },
    "decorate": "function(request, response) { \
      response.body.processed = true; \
      response.body.timestamp = Date.now(); \
      return response; \
    }",
    "wait": 100
  }
}
```

---

## Best Practices

1. **Use wait sparingly** - Only for testing timeout handling
2. **Keep decorate functions simple** - Complex logic is hard to debug
3. **Use copy for echoing** - More maintainable than decorate for simple cases
4. **Test behaviors individually** - Easier to debug
5. **Document behavior purpose** - Future maintainers will thank you

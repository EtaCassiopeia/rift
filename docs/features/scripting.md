---
layout: default
title: Scripting
parent: Features
nav_order: 2
---

# Scripting

Rift supports multiple scripting engines for dynamic behavior.

---

## Available Engines

| Engine | Mode | Use Case |
|:-------|:-----|:---------|
| **JavaScript** | Mountebank | Injection responses, decorate behaviors |
| **Rhai** | Native | Lightweight fault logic |
| **Lua** | Native | High-performance scripting |

---

## JavaScript (Mountebank Mode)

### Injection Responses

```json
{
  "inject": "function(request, state, logger) { \
    return { \
      statusCode: 200, \
      headers: { 'Content-Type': 'application/json' }, \
      body: JSON.stringify({ path: request.path, timestamp: Date.now() }) \
    }; \
  }"
}
```

### Request Object

```javascript
request.method      // "GET", "POST", etc.
request.path        // "/api/users/123"
request.query       // { page: "1", limit: "10" }
request.headers     // { "content-type": "application/json" }
request.body        // Request body (string or parsed object)
```

### State Object

Persist data across requests:

```javascript
function(request, state, logger) {
  // Initialize or increment counter
  state.counter = (state.counter || 0) + 1;

  // Store user-specific data
  var userId = request.headers['X-User-Id'];
  state.users = state.users || {};
  state.users[userId] = { lastSeen: Date.now() };

  return {
    statusCode: 200,
    body: { requestNumber: state.counter }
  };
}
```

### Logger Object

```javascript
function(request, state, logger) {
  logger.debug("Processing request: " + request.path);
  logger.info("User ID: " + request.headers['X-User-Id']);
  logger.warn("Deprecated endpoint called");
  logger.error("Something went wrong");

  return { statusCode: 200 };
}
```

### Decorate Behavior

```json
{
  "_behaviors": {
    "decorate": "function(request, response) { \
      response.headers = response.headers || {}; \
      response.headers['X-Processed-By'] = 'Rift'; \
      response.headers['X-Request-Id'] = request.headers['X-Request-Id'] || 'unknown'; \
      if (typeof response.body === 'object') { \
        response.body.serverTime = new Date().toISOString(); \
      } \
      return response; \
    }"
  }
}
```

---

## Rhai (Native Mode)

Rhai is a lightweight embedded scripting language optimized for Rust.

### Basic Script

```yaml
rules:
  - id: "rhai-fault"
    fault:
      script:
        engine: rhai
        code: |
          // Access request
          let path = request.path;
          let method = request.method;

          // Conditional logic
          if path.contains("admin") {
            #{
              latency_ms: 100,
              inject: true
            }
          } else {
            #{ inject: false }
          }
```

### Available Variables

```rhai
// Request information
request.path        // String: "/api/users"
request.method      // String: "GET"
request.headers     // Map: { "content-type": "application/json" }
request.query       // Map: { "page": "1" }
request.body        // String: request body

// Random functions
rand()              // Float: 0.0 to 1.0
rand(min, max)      // Integer: min to max (inclusive)

// Time functions
timestamp()         // Current timestamp
timestamp().hour()  // Current hour (0-23)
timestamp().minute() // Current minute (0-59)
```

### Flow State

```rhai
// Get value (returns Option)
let value = flow.get("key");
let count = flow.get("counter").unwrap_or(0);

// Set value
flow.set("key", "value");
flow.set("counter", count + 1);

// Set with expiration (seconds)
flow.set("temp_key", "value");
flow.expire("temp_key", 60);

// Delete value
flow.delete("key");

// Check existence
if flow.exists("key") {
  // ...
}
```

### Return Values

```rhai
// Inject latency
#{
  latency_ms: 500,
  inject: true
}

// Inject error
#{
  error_status: 500,
  error_body: "Service unavailable",
  error_headers: #{ "Retry-After": "30" },
  inject: true
}

// No injection
#{ inject: false }

// Combined
#{
  latency_ms: 200,
  error_probability: 0.1,
  error_status: 503,
  inject: true
}
```

---

## Lua (Native Mode)

Lua provides high-performance scripting with pre-compiled bytecode.

### Basic Script

```yaml
rules:
  - id: "lua-fault"
    fault:
      script:
        engine: lua
        code: |
          local path = request.path

          if string.find(path, "slow") then
            return {
              latency_ms = 1000,
              inject = true
            }
          end

          return { inject = false }
```

### Available Variables

```lua
-- Request information
request.path        -- String
request.method      -- String
request.headers     -- Table
request.query       -- Table
request.body        -- String

-- Random
math.random()       -- Float 0.0 to 1.0
math.random(n)      -- Integer 1 to n
math.random(m, n)   -- Integer m to n

-- Time
os.time()           -- Unix timestamp
os.date("*t")       -- Date table with hour, min, sec, etc.
```

### Flow State

```lua
-- Get value
local value = flow.get("key")
local count = flow.get("counter") or 0

-- Set value
flow.set("key", "value")
flow.set("counter", count + 1)

-- Set with expiration
flow.set("temp", "value")
flow.expire("temp", 60)

-- Delete
flow.delete("key")

-- Exists
if flow.exists("key") then
  -- ...
end
```

---

## Script Examples

### Rate Limiting

```rhai
let user_id = request.headers["X-User-Id"];
let key = "rate:" + user_id;
let count = flow.get(key).unwrap_or(0);

flow.set(key, count + 1);
flow.expire(key, 60);  // Reset every minute

if count > 100 {
  #{
    error_status: 429,
    error_body: `{"error": "Rate limit exceeded", "retry_after": 60}`,
    inject: true
  }
} else {
  #{ inject: false }
}
```

### A/B Testing

```rhai
let user_id = request.headers["X-User-Id"];
let bucket_key = "ab_bucket:" + user_id;

// Assign bucket if not exists
let bucket = flow.get(bucket_key);
if bucket == () {
  bucket = if rand() < 0.5 { "A" } else { "B" };
  flow.set(bucket_key, bucket);
}

// Different behavior per bucket
if bucket == "A" {
  #{
    latency_ms: 0,
    inject: true
  }
} else {
  #{
    latency_ms: 100,
    inject: true
  }
}
```

### Retry Simulation

```rhai
let request_id = request.headers["X-Request-Id"];
let attempt_key = "attempt:" + request_id;
let attempts = flow.get(attempt_key).unwrap_or(0);

flow.set(attempt_key, attempts + 1);
flow.expire(attempt_key, 300);

// Fail first 2 attempts
if attempts < 2 {
  #{
    error_status: 503,
    error_body: `{"error": "Temporary failure", "attempt": ${attempts + 1}}`,
    inject: true
  }
} else {
  #{ inject: false }
}
```

---

## Performance Tips

1. **Prefer Rhai/Lua over JavaScript** for high-throughput scenarios
2. **Pre-compute values** outside request path when possible
3. **Use flow state sparingly** - each access has overhead
4. **Keep scripts simple** - complex logic is harder to debug
5. **Use logging judiciously** - excessive logging impacts performance

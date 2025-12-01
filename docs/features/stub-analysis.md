---
layout: default
title: Stub Analysis
parent: Features
nav_order: 5
---

# Stub Analysis

Rift provides automated stub analysis to detect common configuration issues that can lead to unexpected behavior.

---

## Overview

When you create or modify stubs, Rift analyzes them for potential problems:

- **Duplicate IDs** - Multiple stubs with the same identifier
- **Shadowed stubs** - Stubs that will never match due to earlier stubs
- **Catch-all ordering** - Empty predicate stubs that shadow subsequent stubs
- **Exact duplicates** - Stubs with identical predicates

**Note**: This is a **Rift extension**. Mountebank does not provide overlap detection or warnings.

---

## Viewing Warnings

Warnings appear in API responses under the `_rift.warnings` field:

```bash
curl http://localhost:2525/imposters/4545
```

```json
{
  "port": 4545,
  "protocol": "http",
  "stubs": [...],
  "_rift": {
    "warnings": [
      {
        "warningType": "catch_all_not_last",
        "message": "Catch-all stub at index 0 will shadow 2 stub(s) after it",
        "stubIndex": 0
      },
      {
        "warningType": "potentially_shadowed",
        "message": "Stub at index 1 may be shadowed by catch-all stub at index 0",
        "stubIndex": 1,
        "shadowedByIndex": 0
      }
    ]
  }
}
```

Warnings are also logged to the server console when stubs are added or modified.

---

## Warning Types

### duplicate_id

Multiple stubs have the same `id` field:

```json
{
  "stubs": [
    { "id": "user-stub", "predicates": [{"equals": {"path": "/a"}}], "responses": [...] },
    { "id": "user-stub", "predicates": [{"equals": {"path": "/b"}}], "responses": [...] }
  ]
}
```

**Warning**:
```json
{
  "warningType": "duplicate_id",
  "message": "Stub at index 1 has duplicate ID 'user-stub' (same as stub at index 0)",
  "stubIndex": 1,
  "stubId": "user-stub",
  "shadowedByIndex": 0
}
```

### exact_duplicate

Two stubs have identical predicates. The second stub will never match:

```json
{
  "stubs": [
    { "predicates": [{"equals": {"path": "/test"}}], "responses": [{"is": {"body": "first"}}] },
    { "predicates": [{"equals": {"path": "/test"}}], "responses": [{"is": {"body": "second"}}] }
  ]
}
```

**Warning**:
```json
{
  "warningType": "exact_duplicate",
  "message": "Stub at index 1 has identical predicates to stub at index 0 and will never match",
  "stubIndex": 1,
  "shadowedByIndex": 0
}
```

### potentially_shadowed

A stub may be unreachable because an earlier stub matches a superset of requests:

```json
{
  "stubs": [
    { "predicates": [{"startsWith": {"path": "/api"}}], "responses": [...] },
    { "predicates": [{"equals": {"path": "/api/users"}}], "responses": [...] }
  ]
}
```

**Warning**:
```json
{
  "warningType": "potentially_shadowed",
  "message": "Stub at index 1 may be partially shadowed by stub at index 0 which has overlapping predicates",
  "stubIndex": 1,
  "shadowedByIndex": 0
}
```

### catch_all

A stub with empty predicates matches ALL requests:

```json
{
  "stubs": [
    { "predicates": [], "responses": [{"is": {"body": "catch all"}}] }
  ]
}
```

**Warning**:
```json
{
  "warningType": "catch_all",
  "message": "Stub at index 0 has empty predicates and will match ALL requests",
  "stubIndex": 0
}
```

### catch_all_not_last

A catch-all stub appears before other stubs, shadowing them:

```json
{
  "stubs": [
    { "predicates": [], "responses": [{"is": {"body": "catch all"}}] },
    { "predicates": [{"equals": {"path": "/specific"}}], "responses": [{"is": {"body": "specific"}}] }
  ]
}
```

**Warning**:
```json
{
  "warningType": "catch_all_not_last",
  "message": "Catch-all stub at index 0 will shadow 1 stub(s) after it",
  "stubIndex": 0
}
```

---

## Stub ID Field

Rift extends the stub schema with an optional `id` field for easier management:

```json
{
  "stubs": [
    {
      "id": "get-users",
      "predicates": [{"equals": {"method": "GET", "path": "/users"}}],
      "responses": [{"is": {"statusCode": 200, "body": "[]"}}]
    },
    {
      "id": "create-user",
      "predicates": [{"equals": {"method": "POST", "path": "/users"}}],
      "responses": [{"is": {"statusCode": 201}}]
    }
  ]
}
```

Benefits:
- Easier to identify stubs in logs and warnings
- Self-documenting stub configurations
- Future: May support ID-based stub operations

**Note**: The `id` field is ignored by Mountebank but preserved by Rift.

---

## First-Match-Wins Semantics

Both Mountebank and Rift use **first-match-wins** semantics:

1. Stubs are evaluated in order (index 0, 1, 2, ...)
2. The first stub whose predicates match is used
3. Subsequent stubs are not evaluated

### Example

```json
{
  "stubs": [
    {
      "predicates": [{"startsWith": {"path": "/api"}}],
      "responses": [{"is": {"body": "general"}}]
    },
    {
      "predicates": [{"equals": {"path": "/api/users"}}],
      "responses": [{"is": {"body": "specific"}}]
    }
  ]
}
```

| Request | Matches | Response |
|:--------|:--------|:---------|
| GET /api/users | Stub 0 (startsWith /api) | "general" |
| GET /api/orders | Stub 0 (startsWith /api) | "general" |
| GET /other | No match | Default response |

To get "specific" for `/api/users`, swap the stub order:

```json
{
  "stubs": [
    {
      "predicates": [{"equals": {"path": "/api/users"}}],
      "responses": [{"is": {"body": "specific"}}]
    },
    {
      "predicates": [{"startsWith": {"path": "/api"}}],
      "responses": [{"is": {"body": "general"}}]
    }
  ]
}
```

---

## Best Practices

### 1. Order stubs from specific to general

```json
{
  "stubs": [
    { "predicates": [{"equals": {"path": "/api/users/123"}}], ... },
    { "predicates": [{"equals": {"path": "/api/users"}}], ... },
    { "predicates": [{"startsWith": {"path": "/api"}}], ... },
    { "predicates": [], ... }  // Catch-all last
  ]
}
```

### 2. Use unique IDs for each stub

```json
{
  "stubs": [
    { "id": "get-user-by-id", ... },
    { "id": "list-users", ... },
    { "id": "api-fallback", ... }
  ]
}
```

### 3. Place catch-all stubs last

```json
{
  "stubs": [
    { "predicates": [{"equals": {"path": "/health"}}], ... },
    { "predicates": [{"equals": {"path": "/ready"}}], ... },
    { "predicates": [], "responses": [{"is": {"statusCode": 404}}] }  // Last
  ]
}
```

### 4. Check warnings after creating imposters

```bash
# Create imposter
curl -X POST http://localhost:2525/imposters -d @imposter.json

# Check for warnings
curl http://localhost:2525/imposters/4545 | jq '._rift.warnings'
```

### 5. Different method = different stub (no conflict)

Stubs with the same path but different methods don't conflict:

```json
{
  "stubs": [
    { "predicates": [{"equals": {"path": "/users", "method": "GET"}}], ... },
    { "predicates": [{"equals": {"path": "/users", "method": "POST"}}], ... }
  ]
}
```

---

## Mountebank Compatibility

| Feature | Mountebank | Rift |
|:--------|:-----------|:-----|
| First-match-wins | Yes | Yes |
| Overlap detection | No | Yes (warnings) |
| Stub IDs | No | Yes |
| Warning messages | No | Yes |
| Duplicate allowed | Yes | Yes (with warning) |

The `id` field and `_rift.warnings` are Rift extensions that don't affect Mountebank compatibility. Mountebank will ignore the `id` field if present.

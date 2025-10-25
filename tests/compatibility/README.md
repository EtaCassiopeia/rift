# Mountebank vs Rift Compatibility Tests

This directory contains side-by-side compatibility tests to verify that Rift
is a drop-in replacement for Mountebank.

## Overview

The test suite runs both Mountebank and Rift simultaneously and compares:
- Admin API responses
- Imposter behavior
- Predicate matching
- Response behaviors
- Request recording
- Error handling

## Test Frameworks

Two test approaches are available:

### 1. Rust BDD Tests (cucumber-rs) - Recommended

Comprehensive Gherkin-based tests with structured scenarios.

```bash
# Start services
docker compose up -d

# Run BDD tests
cargo test --test compatibility

# Run specific feature
cargo test --test compatibility -- features/predicates.feature

# Run with verbose output
cargo test --test compatibility -- -v

# Stop services
docker compose down
```

### 2. Shell Script Tests (Quick Validation)

Simple bash-based tests for quick validation.

```bash
# Start services
docker compose up -d

# Run shell tests
./run-tests.sh

# Stop services
docker compose down
```

## Prerequisites

- Docker and Docker Compose
- Rust toolchain (for BDD tests)
- curl and jq (for shell tests)

## Port Mapping

| Service    | Admin API | Imposter Port 1 | Imposter Port 2 | Imposter Port 3 |
|------------|-----------|-----------------|-----------------|-----------------|
| Mountebank | 2525      | 4545            | 4546            | 4547            |
| Rift       | 3525      | 5545            | 5546            | 5547            |

Rift also exposes a metrics endpoint on port 9090.

## Feature Files

The BDD tests are organized into feature files in `features/`:

### `admin_api.feature`
- Root endpoint (`GET /`)
- Imposter CRUD operations
- Batch operations (PUT/DELETE /imposters)
- Query parameters (replayable, removeProxies)
- 404 handling

### `predicates.feature`
- `equals` - Exact match on method, path, headers, query, body
- `contains` - Substring matching
- `startsWith` - Prefix matching
- `endsWith` - Suffix matching
- `matches` - Regex pattern matching
- `exists` - Field presence checking
- `and`, `or`, `not` - Compound predicates
- `caseSensitive` - Case sensitivity control
- Multiple predicates (implicit AND)

### `responses.feature`
- Status codes
- Headers
- String and JSON bodies
- Response cycling (multiple responses)
- Wait behavior (delayed responses)
- Repeat behavior
- Decorate behavior (JavaScript modification)
- Copy behavior
- Default responses
- Fault injection

### `recording.feature`
- Request recording when enabled
- Recording disabled
- Request details (method, path, headers, body, query)
- Request counting
- Clearing saved requests
- Timestamp recording
- Multiple imposter recording

### `complex_scenarios.feature`
- Dynamic imposter management workflows
- Stub ordering and priority
- State-based response sequences
- Concurrent request handling
- Large payloads
- Unicode and special characters
- Error recovery
- Import/export workflows

## Writing New Tests

### Gherkin Syntax

```gherkin
Feature: My New Feature
  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  Scenario: Test something
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"equals": {"path": "/test"}}],
        "responses": [{"is": {"statusCode": 200, "body": "ok"}}]
      }
      """
    When I send GET request to "/test" on imposter 4545
    Then both services should return status 200
    And both responses should have body "ok"
```

### Available Step Definitions

**Given steps:**
- `both Mountebank and Rift services are running`
- `all imposters are cleared`
- `an imposter exists on port {port}`
- `an imposter on port {port} with stub: {json}`
- `an imposter on port {port} with recordRequests enabled`

**When steps:**
- `I send {METHOD} request to "{path}" on both services`
- `I send {METHOD} request to "{path}" on imposter {port}`
- `I create an imposter on both services: {json}`
- `I send {count} GET requests to "{path}" on imposter {port}`

**Then steps:**
- `both services should return status {code}`
- `both responses should have body "{expected}"`
- `both responses should have header "{name}" with value "{value}"`
- `both services should have recorded {count} requests`

## Test Configuration

Test imposter configurations are in `configs/`:
- `basic-imposters.json` - Standard test imposters

## Troubleshooting

**Services not starting:**
```bash
docker compose logs -f
```

**Check service health:**
```bash
curl http://localhost:2525/  # Mountebank
curl http://localhost:3525/  # Rift
```

**Rebuild Rift image:**
```bash
docker compose build rift
```

**Run tests with debug logging:**
```bash
RUST_LOG=debug cargo test --test compatibility
```

## CI/CD Integration

The tests can be run in CI with:

```yaml
- name: Start services
  run: docker compose -f tests/compatibility/docker-compose.yml up -d

- name: Wait for services
  run: |
    timeout 60 bash -c 'until curl -s http://localhost:2525/ && curl -s http://localhost:3525/; do sleep 1; done'

- name: Run compatibility tests
  run: cargo test --test compatibility

- name: Stop services
  run: docker compose -f tests/compatibility/docker-compose.yml down
```

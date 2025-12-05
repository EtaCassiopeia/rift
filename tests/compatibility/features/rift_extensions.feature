Feature: Rift Extensions (_rift namespace)
  Tests for Rift-specific extensions that enhance Mountebank functionality
  Note: These tests run against Rift only (not Mountebank) as they test Rift-specific features

  Background:
    Given Rift service is running
    And all imposters are cleared on Rift

  # ==========================================================================
  # Flow State Extensions
  # ==========================================================================

  @rift-only
  Scenario: Flow state with inject response maintains counter
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {
            "backend": "inmemory",
            "ttlSeconds": 300
          }
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { state.count = (state.count || 0) + 1; return { statusCode: 200, body: 'Count: ' + state.count }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "Count: 1"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "Count: 2"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "Count: 3"

  @rift-only
  Scenario: Flow state persists data across different endpoints
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory"}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"method": "POST", "path": "/store"}}],
            "responses": [{
              "inject": "function(request, state) { var data = JSON.parse(request.body); state.value = data.value; return { statusCode: 201, body: 'Stored' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"method": "GET", "path": "/retrieve"}}],
            "responses": [{
              "inject": "function(request, state) { return { statusCode: 200, body: state.value || 'empty' }; }"
            }]
          }
        ]
      }
      """
    When I send GET request to "/retrieve" on Rift imposter 4545
    Then Rift response body should be "empty"
    When I send POST request with body '{"value": "test-data"}' to "/store" on Rift imposter 4545
    Then Rift should return status 201
    When I send GET request to "/retrieve" on Rift imposter 4545
    Then Rift response body should be "test-data"

  # ==========================================================================
  # Fault Injection Extensions
  # ==========================================================================

  @rift-only
  Scenario: Latency fault with fixed delay
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {"statusCode": 200, "body": "delayed"},
            "_rift": {
              "fault": {
                "latency": {
                  "probability": 1.0,
                  "ms": 200
                }
              }
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545 and measure time
    Then Rift should return status 200
    And Rift response should take at least 180ms

  @rift-only
  Scenario: Latency fault with range
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {"statusCode": 200, "body": "delayed"},
            "_rift": {
              "fault": {
                "latency": {
                  "probability": 1.0,
                  "minMs": 100,
                  "maxMs": 200
                }
              }
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545 and measure time
    Then Rift should return status 200
    And Rift response should take at least 90ms
    And Rift response should take at most 300ms

  @rift-only
  Scenario: Error fault with 100% probability
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {"statusCode": 200, "body": "normal"},
            "_rift": {
              "fault": {
                "error": {
                  "probability": 1.0,
                  "status": 503,
                  "body": "Service Unavailable"
                }
              }
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response body should be "Service Unavailable"

  @rift-only
  Scenario: Error fault with custom headers
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {"statusCode": 200, "body": "normal"},
            "_rift": {
              "fault": {
                "error": {
                  "probability": 1.0,
                  "status": 429,
                  "body": "Too Many Requests",
                  "headers": {
                    "Retry-After": "60",
                    "X-RateLimit-Reset": "1234567890"
                  }
                }
              }
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 429
    And Rift response should have header "Retry-After" with value "60"
    And Rift response should have header "X-RateLimit-Reset" with value "1234567890"

  # ==========================================================================
  # Combined Mountebank and Rift Extensions
  # ==========================================================================

  @rift-only
  Scenario: Mountebank _behaviors combined with _rift fault
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {
              "statusCode": 200,
              "headers": {"X-Custom": "value"},
              "body": "combined response"
            },
            "_behaviors": {"wait": 50},
            "_rift": {
              "fault": {
                "latency": {"probability": 1.0, "ms": 50}
              }
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545 and measure time
    Then Rift should return status 200
    And Rift response should have header "X-Custom" with value "value"
    And Rift response should take at least 80ms

  @rift-only
  Scenario: Mountebank predicates work with _rift extensions
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory"}
        },
        "stubs": [
          {
            "predicates": [
              {"equals": {"method": "GET"}},
              {"startsWith": {"path": "/api/"}}
            ],
            "responses": [{
              "is": {"statusCode": 200, "body": "API response"},
              "_rift": {"fault": {"latency": {"probability": 1.0, "ms": 10}}}
            }]
          },
          {
            "predicates": [{"equals": {"method": "POST"}}],
            "responses": [{"is": {"statusCode": 201, "body": "Created"}}]
          }
        ]
      }
      """
    When I send GET request to "/api/users" on Rift imposter 4545 and measure time
    Then Rift should return status 200
    And Rift response body should be "API response"
    When I send POST request to "/create" on Rift imposter 4545
    Then Rift should return status 201
    And Rift response body should be "Created"

  @rift-only
  Scenario: Response cycling with _rift extensions
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [
            {"is": {"statusCode": 200, "body": "first"}, "_rift": {"fault": {"latency": {"probability": 1.0, "ms": 5}}}},
            {"is": {"statusCode": 200, "body": "second"}},
            {"is": {"statusCode": 200, "body": "third"}, "_rift": {"fault": {"latency": {"probability": 1.0, "ms": 5}}}}
          ]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "first"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "second"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "third"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "first"

  @rift-only
  Scenario: Default response with _rift config
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory"}
        },
        "defaultResponse": {
          "statusCode": 404,
          "body": "Not found"
        },
        "stubs": [{
          "predicates": [{"equals": {"path": "/exists"}}],
          "responses": [{"is": {"statusCode": 200, "body": "Found"}}]
        }]
      }
      """
    When I send GET request to "/exists" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "Found"
    When I send GET request to "/nonexistent" on Rift imposter 4545
    Then Rift should return status 404
    And Rift response body should be "Not found"

  # ==========================================================================
  # Backward Compatibility
  # ==========================================================================

  @rift-only
  Scenario: Standard Mountebank config works without _rift extensions
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "recordRequests": true,
        "stubs": [{
          "predicates": [{"equals": {"path": "/test"}}],
          "responses": [{
            "is": {
              "statusCode": 200,
              "headers": {"Content-Type": "application/json"},
              "body": "{\"success\": true}"
            },
            "_behaviors": {"wait": 10}
          }]
        }]
      }
      """
    When I send GET request to "/test" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "Content-Type" with value "application/json"

  @rift-only
  Scenario: Empty _rift config is ignored
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {},
        "stubs": [{
          "predicates": [],
          "responses": [{
            "is": {"statusCode": 200, "body": "works"},
            "_rift": {}
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "works"

  # ==========================================================================
  # Advanced Scenarios - Circuit Breaker Pattern
  # ==========================================================================

  @rift-only @advanced
  Scenario: Circuit breaker opens after consecutive failures
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var failures = state.failures || 0; var circuitOpen = state.circuitOpen || false; var threshold = 3; if (circuitOpen) { return { statusCode: 503, body: 'Circuit OPEN' }; } if (request.query && request.query.fail === 'true') { failures++; state.failures = failures; if (failures >= threshold) { state.circuitOpen = true; } return { statusCode: 500, body: 'Failure ' + failures }; } state.failures = 0; return { statusCode: 200, body: 'Success' }; }"
          }]
        }]
      }
      """
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift should return status 500
    And Rift response body should be "Failure 1"
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift response body should be "Failure 2"
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift response body should be "Failure 3"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response body should be "Circuit OPEN"

  @rift-only @advanced
  Scenario: Circuit breaker resets after success
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var failures = state.failures || 0; if (request.query && request.query.fail === 'true') { failures++; state.failures = failures; return { statusCode: 500, body: 'Failures: ' + failures }; } var prev = failures; state.failures = 0; return { statusCode: 200, body: 'Reset from ' + prev + ' to 0' }; }"
          }]
        }]
      }
      """
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift response body should be "Failures: 1"
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift response body should be "Failures: 2"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "Reset from 2 to 0"
    When I send GET request to "/?fail=true" on Rift imposter 4545
    Then Rift response body should be "Failures: 1"

  # ==========================================================================
  # Advanced Scenarios - Rate Limiting
  # ==========================================================================

  @rift-only @advanced
  Scenario: Token bucket rate limiter
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var tokens = state.tokens; if (tokens === undefined) { tokens = 3; } if (tokens > 0) { state.tokens = tokens - 1; return { statusCode: 200, body: 'OK, tokens remaining: ' + (tokens - 1) }; } return { statusCode: 429, headers: {'Retry-After': '60'}, body: 'Rate limited' }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "OK, tokens remaining: 2"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "OK, tokens remaining: 1"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift response body should be "OK, tokens remaining: 0"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 429
    And Rift response should have header "Retry-After" with value "60"

  @rift-only @advanced
  Scenario: Per-client rate limiting using headers
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var clientId = (request.headers && request.headers['X-Client-Id']) || 'anonymous'; var key = 'tokens_' + clientId; var tokens = state[key]; if (tokens === undefined) { tokens = 2; } if (tokens > 0) { state[key] = tokens - 1; return { statusCode: 200, body: clientId + ' OK, remaining: ' + (tokens - 1) }; } return { statusCode: 429, body: clientId + ' rate limited' }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" with header "X-Client-ID: client-a" on Rift imposter 4545
    Then Rift response body should be "client-a OK, remaining: 1"
    When I send GET request to "/" with header "X-Client-ID: client-b" on Rift imposter 4545
    Then Rift response body should be "client-b OK, remaining: 1"
    When I send GET request to "/" with header "X-Client-ID: client-a" on Rift imposter 4545
    Then Rift response body should be "client-a OK, remaining: 0"
    When I send GET request to "/" with header "X-Client-ID: client-a" on Rift imposter 4545
    Then Rift should return status 429
    When I send GET request to "/" with header "X-Client-ID: client-b" on Rift imposter 4545
    Then Rift should return status 200

  # ==========================================================================
  # Advanced Scenarios - Retry and Backoff
  # ==========================================================================

  @rift-only @advanced
  Scenario: Retry counter with eventual success
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var attempts = (state.attempts || 0) + 1; state.attempts = attempts; var successAfter = 3; if (attempts < successAfter) { return { statusCode: 503, headers: {'X-Retry-Attempt': String(attempts)}, body: 'Try again, attempt ' + attempts }; } state.attempts = 0; return { statusCode: 200, body: 'Success on attempt ' + attempts }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response should have header "X-Retry-Attempt" with value "1"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response should have header "X-Retry-Attempt" with value "2"
    When I send GET request to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "Success on attempt 3"

  # ==========================================================================
  # Advanced Scenarios - Session Affinity
  # ==========================================================================

  @rift-only @advanced
  Scenario: Session affinity routes to consistent backend
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 300}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var sessionId = (request.headers && request.headers['X-Session-Id']) || 'default'; var backends = ['backend-1', 'backend-2', 'backend-3']; var key = 'session_' + sessionId; var assigned = state[key]; if (!assigned) { var hash = 0; for (var i = 0; i < sessionId.length; i++) { hash = ((hash << 5) - hash) + sessionId.charCodeAt(i); hash = hash & hash; } assigned = backends[Math.abs(hash) % backends.length]; state[key] = assigned; } return { statusCode: 200, body: 'Routed to: ' + assigned }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" with header "X-Session-ID: user-123" on Rift imposter 4545
    Then Rift should return status 200
    And I save the Rift response body as "first_backend"
    When I send GET request to "/" with header "X-Session-ID: user-123" on Rift imposter 4545
    Then Rift response body should match saved "first_backend"
    When I send GET request to "/" with header "X-Session-ID: user-123" on Rift imposter 4545
    Then Rift response body should match saved "first_backend"

  # ==========================================================================
  # Advanced Scenarios - Cascading Failures
  # ==========================================================================

  @rift-only @advanced
  Scenario: Cascading failure simulation across services
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/database"}}],
            "responses": [{
              "inject": "function(request, state) { if (state.dbDown) { return { statusCode: 503, body: 'Database unavailable' }; } return { statusCode: 200, body: 'Database OK' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/cache"}}],
            "responses": [{
              "inject": "function(request, state) { if (state.dbDown) { state.cacheDown = true; return { statusCode: 503, body: 'Cache failed (DB dependency)' }; } return { statusCode: 200, body: 'Cache OK' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/api"}}],
            "responses": [{
              "inject": "function(request, state) { if (state.cacheDown || state.dbDown) { return { statusCode: 503, body: 'API unavailable (cascade)' }; } return { statusCode: 200, body: 'API OK' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/trigger-failure"}}],
            "responses": [{
              "inject": "function(request, state) { state.dbDown = true; return { statusCode: 200, body: 'Database failure triggered' }; }"
            }]
          }
        ]
      }
      """
    When I send GET request to "/database" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should be "Database OK"
    When I send GET request to "/cache" on Rift imposter 4545
    Then Rift response body should be "Cache OK"
    When I send GET request to "/api" on Rift imposter 4545
    Then Rift response body should be "API OK"
    When I send GET request to "/trigger-failure" on Rift imposter 4545
    Then Rift response body should be "Database failure triggered"
    When I send GET request to "/database" on Rift imposter 4545
    Then Rift should return status 503
    When I send GET request to "/cache" on Rift imposter 4545
    Then Rift should return status 503
    When I send GET request to "/api" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response body should be "API unavailable (cascade)"

  # ==========================================================================
  # Advanced Scenarios - Request Deduplication
  # ==========================================================================

  @rift-only @advanced
  Scenario: Idempotent request handling with deduplication
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 300}
        },
        "stubs": [{
          "predicates": [{"equals": {"method": "POST"}}],
          "responses": [{
            "inject": "function(request, state) { var idempotencyKey = request.headers && request.headers['Idempotency-Key']; if (!idempotencyKey) { return { statusCode: 400, body: 'Missing Idempotency-Key' }; } var key = 'req_' + idempotencyKey; var existing = state[key]; if (existing) { return { statusCode: 200, headers: {'X-Idempotent-Replay': 'true'}, body: existing }; } var result = 'Created resource ' + idempotencyKey; state[key] = result; return { statusCode: 201, body: result }; }"
          }]
        }]
      }
      """
    When I send POST request with header "Idempotency-Key: req-001" to "/" on Rift imposter 4545
    Then Rift should return status 201
    And Rift response body should be "Created resource req-001"
    When I send POST request with header "Idempotency-Key: req-001" to "/" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "X-Idempotent-Replay" with value "true"
    And Rift response body should be "Created resource req-001"
    When I send POST request with header "Idempotency-Key: req-002" to "/" on Rift imposter 4545
    Then Rift should return status 201
    And Rift response body should be "Created resource req-002"

  # ==========================================================================
  # Advanced Scenarios - A/B Testing
  # ==========================================================================

  @rift-only @advanced
  Scenario: A/B testing with percentage-based routing
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 300}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var userId = (request.headers && request.headers['X-User-Id']) || 'anonymous'; var key = 'variant_' + userId; var variant = state[key]; if (!variant) { var hash = 0; for (var i = 0; i < userId.length; i++) { hash = ((hash << 5) - hash) + userId.charCodeAt(i); } var bucket = Math.abs(hash) % 100; variant = bucket < 30 ? 'A' : 'B'; state[key] = variant; } return { statusCode: 200, headers: {'X-Variant': variant}, body: 'Variant ' + variant + ' for ' + userId }; }"
          }]
        }]
      }
      """
    When I send GET request to "/" with header "X-User-ID: test-user-1" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "X-Variant"
    When I send GET request to "/" with header "X-User-ID: test-user-1" on Rift imposter 4545
    Then Rift response should have same header "X-Variant" as previous request

  # ==========================================================================
  # Advanced Scenarios - Gradual Rollout
  # ==========================================================================

  @rift-only @advanced
  Scenario: Feature flag gradual rollout
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/set-rollout"}}],
            "responses": [{
              "inject": "function(request, state) { var pct = parseInt(request.query && request.query.percent) || 0; state.rolloutPercent = pct; return { statusCode: 200, body: 'Rollout set to ' + pct + '%' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/feature"}}],
            "responses": [{
              "inject": "function(request, state) { var userId = (request.headers && request.headers['X-User-Id']) || 'anon'; var pct = state.rolloutPercent || 0; var hash = 0; for (var i = 0; i < userId.length; i++) { hash = ((hash << 5) - hash) + userId.charCodeAt(i); } var bucket = Math.abs(hash) % 100; var enabled = bucket < pct; return { statusCode: 200, headers: {'X-Feature-Enabled': String(enabled)}, body: enabled ? 'New feature' : 'Old feature' }; }"
            }]
          }
        ]
      }
      """
    When I send GET request to "/set-rollout?percent=0" on Rift imposter 4545
    Then Rift response body should be "Rollout set to 0%"
    When I send GET request to "/feature" with header "X-User-ID: user1" on Rift imposter 4545
    Then Rift response body should be "Old feature"
    When I send GET request to "/set-rollout?percent=100" on Rift imposter 4545
    Then Rift response body should be "Rollout set to 100%"
    When I send GET request to "/feature" with header "X-User-ID: user1" on Rift imposter 4545
    Then Rift response body should be "New feature"

  # ==========================================================================
  # Advanced Scenarios - Saga Pattern
  # ==========================================================================

  @rift-only @advanced
  Scenario: Saga pattern with compensation on failure
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 300}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/saga/start"}}],
            "responses": [{
              "inject": "function(request, state) { var sagaId = 'saga_' + Date.now(); state[sagaId] = {steps: [], status: 'running'}; state.currentSaga = sagaId; return { statusCode: 200, headers: {'X-Saga-ID': sagaId}, body: 'Saga started' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/saga/step"}}],
            "responses": [{
              "inject": "function(request, state) { var sagaId = state.currentSaga; if (!sagaId || !state[sagaId]) { return { statusCode: 400, body: 'No active saga' }; } var stepName = (request.query && request.query.name) || 'step'; var shouldFail = request.query && request.query.fail === 'true'; if (shouldFail) { state[sagaId].status = 'failed'; return { statusCode: 500, body: 'Step ' + stepName + ' failed, saga rolled back' }; } state[sagaId].steps.push(stepName); return { statusCode: 200, body: 'Step ' + stepName + ' completed. Total steps: ' + state[sagaId].steps.length }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/saga/status"}}],
            "responses": [{
              "inject": "function(request, state) { var sagaId = state.currentSaga; if (!sagaId || !state[sagaId]) { return { statusCode: 404, body: 'No saga found' }; } var saga = state[sagaId]; return { statusCode: 200, body: 'Status: ' + saga.status + ', Steps: ' + saga.steps.join(',') }; }"
            }]
          }
        ]
      }
      """
    When I send POST request to "/saga/start" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "X-Saga-ID"
    When I send POST request to "/saga/step?name=reserve-inventory" on Rift imposter 4545
    Then Rift response body should be "Step reserve-inventory completed. Total steps: 1"
    When I send POST request to "/saga/step?name=charge-payment" on Rift imposter 4545
    Then Rift response body should be "Step charge-payment completed. Total steps: 2"
    When I send GET request to "/saga/status" on Rift imposter 4545
    Then Rift response body should contain "Steps: reserve-inventory,charge-payment"

  @rift-only @advanced
  Scenario: Saga compensation on failure
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 300}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/saga/start"}}],
            "responses": [{
              "inject": "function(request, state) { state.sagaSteps = []; state.sagaStatus = 'running'; return { statusCode: 200, body: 'Saga started' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/saga/step"}}],
            "responses": [{
              "inject": "function(request, state) { if (state.sagaStatus !== 'running') { return { statusCode: 400, body: 'Saga not running' }; } var stepName = (request.query && request.query.name) || 'step'; var shouldFail = request.query && request.query.fail === 'true'; if (shouldFail) { state.sagaStatus = 'compensating'; var compensated = state.sagaSteps.reverse().map(function(s) { return 'undo-' + s; }); state.compensationLog = compensated; return { statusCode: 500, body: 'Failed at ' + stepName + '. Compensating: ' + compensated.join(', ') }; } state.sagaSteps.push(stepName); return { statusCode: 200, body: 'Completed: ' + stepName }; }"
            }]
          }
        ]
      }
      """
    When I send POST request to "/saga/start" on Rift imposter 4545
    Then Rift should return status 200
    When I send POST request to "/saga/step?name=step1" on Rift imposter 4545
    Then Rift response body should be "Completed: step1"
    When I send POST request to "/saga/step?name=step2" on Rift imposter 4545
    Then Rift response body should be "Completed: step2"
    When I send POST request to "/saga/step?name=step3&fail=true" on Rift imposter 4545
    Then Rift should return status 500
    And Rift response body should contain "Compensating: undo-step2, undo-step1"

  # ==========================================================================
  # Advanced Scenarios - Health Check with Dependencies
  # ==========================================================================

  @rift-only @advanced
  Scenario: Health check aggregating multiple dependency statuses
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/health"}}],
            "responses": [{
              "inject": "function(request, state) { var deps = {database: state.dbHealthy !== false, cache: state.cacheHealthy !== false, queue: state.queueHealthy !== false}; var allHealthy = deps.database && deps.cache && deps.queue; var status = allHealthy ? 200 : 503; var body = JSON.stringify({status: allHealthy ? 'healthy' : 'unhealthy', dependencies: deps}); return { statusCode: status, headers: {'Content-Type': 'application/json'}, body: body }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/set-health"}}],
            "responses": [{
              "inject": "function(request, state) { var service = request.query && request.query.service; var healthy = request.query && request.query.healthy === 'true'; if (service === 'database') state.dbHealthy = healthy; if (service === 'cache') state.cacheHealthy = healthy; if (service === 'queue') state.queueHealthy = healthy; return { statusCode: 200, body: service + ' set to ' + (healthy ? 'healthy' : 'unhealthy') }; }"
            }]
          }
        ]
      }
      """
    When I send GET request to "/health" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response body should contain "healthy"
    When I send GET request to "/set-health?service=database&healthy=false" on Rift imposter 4545
    Then Rift response body should be "database set to unhealthy"
    When I send GET request to "/health" on Rift imposter 4545
    Then Rift should return status 503
    And Rift response body should contain "unhealthy"

  # ==========================================================================
  # Advanced Scenarios - Request Coalescing
  # ==========================================================================

  @rift-only @advanced
  Scenario: Request coalescing for duplicate in-flight requests
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [{
          "predicates": [],
          "responses": [{
            "inject": "function(request, state) { var resourceId = request.query && request.query.id; if (!resourceId) { return { statusCode: 400, body: 'Missing id' }; } var key = 'resource_' + resourceId; var cached = state[key]; if (cached) { return { statusCode: 200, headers: {'X-Cache': 'HIT'}, body: cached }; } var result = 'Data for ' + resourceId + ' (fetched at ' + Date.now() + ')'; state[key] = result; return { statusCode: 200, headers: {'X-Cache': 'MISS'}, body: result }; }"
          }]
        }]
      }
      """
    When I send GET request to "/?id=123" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "X-Cache" with value "MISS"
    When I send GET request to "/?id=123" on Rift imposter 4545
    Then Rift response should have header "X-Cache" with value "HIT"
    When I send GET request to "/?id=456" on Rift imposter 4545
    Then Rift response should have header "X-Cache" with value "MISS"

  # ==========================================================================
  # Advanced Scenarios - Multi-Region Failover
  # ==========================================================================

  @rift-only @advanced
  Scenario: Multi-region failover simulation
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "_rift": {
          "flowState": {"backend": "inmemory", "ttlSeconds": 60}
        },
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/api"}}],
            "responses": [{
              "inject": "function(request, state) { var regions = ['us-east', 'us-west', 'eu-west']; var failedRegions = state.failedRegions || []; for (var i = 0; i < regions.length; i++) { var region = regions[i]; if (failedRegions.indexOf(region) === -1) { return { statusCode: 200, headers: {'X-Served-By': region}, body: 'Response from ' + region }; } } return { statusCode: 503, body: 'All regions unavailable' }; }"
            }]
          },
          {
            "predicates": [{"equals": {"path": "/fail-region"}}],
            "responses": [{
              "inject": "function(request, state) { var region = request.query && request.query.region; if (!region) { return { statusCode: 400, body: 'Missing region' }; } var failed = state.failedRegions || []; if (failed.indexOf(region) === -1) { failed.push(region); state.failedRegions = failed; } return { statusCode: 200, body: region + ' marked as failed' }; }"
            }]
          }
        ]
      }
      """
    When I send GET request to "/api" on Rift imposter 4545
    Then Rift should return status 200
    And Rift response should have header "X-Served-By" with value "us-east"
    When I send GET request to "/fail-region?region=us-east" on Rift imposter 4545
    Then Rift response body should be "us-east marked as failed"
    When I send GET request to "/api" on Rift imposter 4545
    Then Rift response should have header "X-Served-By" with value "us-west"
    When I send GET request to "/fail-region?region=us-west" on Rift imposter 4545
    Then Rift response body should be "us-west marked as failed"
    When I send GET request to "/api" on Rift imposter 4545
    Then Rift response should have header "X-Served-By" with value "eu-west"

  # ==========================================================================
  # Script Validation at Configuration Time
  # ==========================================================================

  @rift-only
  Scenario: Rhai script with syntax error is rejected at creation time
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "_rift": {
              "script": {
                "engine": "rhai",
                "code": "fn should_inject(request, flow_store) { #{ inject: "
              }
            }
          }]
        }]
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"
    And Rift response body should contain "Syntax error"

  @rift-only
  Scenario: Rhai script missing should_inject function is rejected
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "_rift": {
              "script": {
                "engine": "rhai",
                "code": "fn wrong_function_name(x) { x + 1 }"
              }
            }
          }]
        }]
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"
    And Rift response body should contain "should_inject"

  @rift-only
  Scenario: Lua script with syntax error is rejected at creation time
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "_rift": {
              "script": {
                "engine": "lua",
                "code": "function should_inject(request, flow_store) return { inject ="
              }
            }
          }]
        }]
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"

  @rift-only
  Scenario: JavaScript inject script with syntax error is rejected at creation time
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "inject": "function(config, state) { return { statusCode: "
          }]
        }]
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"
    And Rift response body should contain "Syntax error"

  @rift-only
  Scenario: Valid Rhai script is accepted
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "_rift": {
              "script": {
                "engine": "rhai",
                "code": "fn should_inject(request, flow_store) { #{ inject: false } }"
              }
            }
          }]
        }]
      }
      """
    Then Rift should return status 201

  @rift-only
  Scenario: Invalid script in stub addition is rejected
    Given an imposter on port 4545 on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{"is": {"statusCode": 200, "body": "OK"}}]
        }]
      }
      """
    When I try to add a stub to imposter 4545 on Rift with:
      """
      {
        "stub": {
          "responses": [{
            "_rift": {
              "script": {
                "engine": "rhai",
                "code": "fn invalid_syntax { }"
              }
            }
          }]
        }
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"

  @rift-only
  Scenario: Unknown script engine is rejected
    When I try to create an imposter on Rift with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "_rift": {
              "script": {
                "engine": "unknown_engine",
                "code": "some code"
              }
            }
          }]
        }]
      }
      """
    Then Rift should return status 400
    And Rift response body should contain "Script validation failed"
    And Rift response body should contain "Unknown script engine"

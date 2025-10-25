Feature: Complex Multi-Step Scenarios
  Advanced scenarios testing complex interactions and edge cases

  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  # ==========================================================================
  # Dynamic Imposter Management
  # ==========================================================================

  Scenario: Create, modify, and delete imposter workflow
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "defaultResponse": {"statusCode": 404, "body": "not found"},
        "stubs": []
      }
      """
    Then both services should return status 201

    When I add a stub to imposter 4545 on both services:
      """
      {
        "predicates": [{"equals": {"path": "/v1"}}],
        "responses": [{"is": {"statusCode": 200, "body": "version 1"}}]
      }
      """
    Then GET "/v1" on imposter 4545 should return "version 1" on both

    When I replace stub 0 on imposter 4545 on both services:
      """
      {
        "predicates": [{"equals": {"path": "/v2"}}],
        "responses": [{"is": {"statusCode": 200, "body": "version 2"}}]
      }
      """
    Then GET "/v2" on imposter 4545 should return "version 2" on both
    And GET "/v1" on imposter 4545 should return 404 on both

    When I send DELETE request to "/imposters/4545" on both services
    Then both services should return status 200

  # ==========================================================================
  # Stub Ordering and Priority
  # ==========================================================================

  Scenario: First matching stub wins
    Given an imposter on port 4545 with stubs:
      """
      [
        {
          "predicates": [{"startsWith": {"path": "/api"}}],
          "responses": [{"is": {"statusCode": 200, "body": "general api"}}]
        },
        {
          "predicates": [{"equals": {"path": "/api/specific"}}],
          "responses": [{"is": {"statusCode": 200, "body": "specific api"}}]
        }
      ]
      """
    When I send GET request to "/api/specific" on imposter 4545
    Then both services should return status 200
    And both responses should have body "general api"

  Scenario: Add stub at specific index
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"startsWith": {"path": "/api"}}],
        "responses": [{"is": {"statusCode": 200, "body": "general"}}]
      }
      """
    When I add a stub at index 0 to imposter 4545 on both services:
      """
      {
        "predicates": [{"equals": {"path": "/api/priority"}}],
        "responses": [{"is": {"statusCode": 200, "body": "priority"}}]
      }
      """
    Then GET "/api/priority" on imposter 4545 should return "priority" on both

  # ==========================================================================
  # State-Based Testing
  # ==========================================================================

  Scenario: Stateful response sequence
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"equals": {"path": "/order"}}],
        "responses": [
          {"is": {"statusCode": 202, "body": "pending"}},
          {"is": {"statusCode": 200, "body": "processing"}},
          {"is": {"statusCode": 200, "body": "completed"}}
        ]
      }
      """
    When I send GET request to "/order" on imposter 4545
    Then both responses should have body "pending"
    When I send GET request to "/order" on imposter 4545
    Then both responses should have body "processing"
    When I send GET request to "/order" on imposter 4545
    Then both responses should have body "completed"
    When I send GET request to "/order" on imposter 4545
    Then both responses should have body "pending"

  # ==========================================================================
  # Concurrent Requests
  # ==========================================================================

  Scenario: Handle concurrent requests
    Given an imposter on port 4545 with recordRequests enabled and stub:
      """
      {
        "predicates": [],
        "responses": [{"is": {"statusCode": 200, "body": "ok"}}]
      }
      """
    When I send 10 concurrent GET requests to "/concurrent" on imposter 4545
    Then all requests should succeed on both services
    And imposter 4545 should have numberOfRequests equal to 10 on both services

  # ==========================================================================
  # Large Payloads
  # ==========================================================================

  Scenario: Handle large request body
    Given an imposter on port 4545 with recordRequests enabled and stub:
      """
      {
        "predicates": [],
        "responses": [{"is": {"statusCode": 200}}]
      }
      """
    When I send POST request with 100KB body on imposter 4545
    Then both services should return status 200
    And recorded request body size should match on both services

  Scenario: Return large response body
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [],
        "responses": [{"is": {"statusCode": 200, "body": "<LARGE_BODY_PLACEHOLDER>"}}]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return status 200
    And response body sizes should match

  # ==========================================================================
  # Special Characters and Encoding
  # ==========================================================================

  Scenario: Handle unicode in path
    # Note: HTTP paths are URL-encoded, so we use URL-encoded path in predicate
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"equals": {"path": "/users/%E6%97%A5%E6%9C%AC%E8%AA%9E"}}],
        "responses": [{"is": {"statusCode": 200, "body": "unicode path"}}]
      }
      """
    When I send GET request to "/users/日本語" on imposter 4545
    Then both services should return status 200
    And both responses should have body "unicode path"

  Scenario: Handle special characters in headers
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [],
        "responses": [{
          "is": {
            "statusCode": 200,
            "headers": {"X-Special": "value with spaces & symbols!"}
          }
        }]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both responses should have header "X-Special" with value "value with spaces & symbols!"

  Scenario: Handle JSON with escaped characters
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [],
        "responses": [{
          "is": {
            "statusCode": 200,
            "body": {"message": "Hello \"World\"", "path": "C:\\Users\\test"}
          }
        }]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return status 200
    And both responses should contain "Hello \"World\""

  # ==========================================================================
  # Error Conditions
  # ==========================================================================

  Scenario: Recover from invalid stub addition
    Given an imposter on port 4545 with stubs:
      """
      [{
        "predicates": [],
        "responses": [{"is": {"statusCode": 200, "body": "original"}}]
      }]
      """
    When I try to add an invalid stub to imposter 4545
    Then both services should return error status
    And GET "/" on imposter 4545 should still return "original" on both

  Scenario: Port conflict handling
    Given an imposter exists on port 4545
    When I try to create another imposter on port 4545 on both services
    Then both services should return status 400 or similar error
    And original imposter should still function

  # ==========================================================================
  # Proxy Mode (if supported)
  # ==========================================================================

  Scenario: Proxy mode forwards unmatched requests
    Given an imposter on port 4545 with proxy to backend:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/local"}}],
            "responses": [{"is": {"statusCode": 200, "body": "local response"}}]
          }
        ],
        "defaultResponse": {
          "proxy": {"to": "http://httpbin.org"}
        }
      }
      """
    When I send GET request to "/local" on imposter 4545
    Then both responses should have body "local response"

  # ==========================================================================
  # Batch Import/Export
  # ==========================================================================

  Scenario: Export and reimport imposters
    Given imposters exist on ports 4545, 4546 with various stubs
    When I export imposters with replayable=true from both services
    Then exported JSON should be valid and equivalent

    When I delete all imposters on both services
    And I reimport the exported configuration
    Then all imposters should be restored identically

  # ==========================================================================
  # Stub Matching with Request Recording
  # ==========================================================================

  Scenario: Verify which stub matched a request
    Given an imposter on port 4545 with recordRequests enabled and multiple stubs:
      """
      [
        {
          "predicates": [{"equals": {"path": "/a"}}],
          "responses": [{"is": {"statusCode": 200, "body": "matched a"}}]
        },
        {
          "predicates": [{"equals": {"path": "/b"}}],
          "responses": [{"is": {"statusCode": 200, "body": "matched b"}}]
        }
      ]
      """
    When I send GET request to "/b" on imposter 4545
    Then both services should return "matched b"
    And stub match count should be updated correctly on both services

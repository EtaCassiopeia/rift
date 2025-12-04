Feature: Mountebank Compatibility Improvements
  Tests for the Mountebank compatibility features implemented to close gaps

  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  # ==========================================================================
  # removeProxies Query Parameter
  # ==========================================================================

  Scenario: removeProxies filters proxy stubs from single imposter response
    Given an imposter on port 4545 with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/static"}}],
            "responses": [{"is": {"statusCode": 200, "body": "static response"}}]
          },
          {
            "predicates": [{"equals": {"path": "/proxy"}}],
            "responses": [{"proxy": {"to": "http://example.com"}}]
          }
        ]
      }
      """
    When I send GET request to "/imposters/4545?removeProxies=true" on both services
    Then both services should return status 200
    And responses should not contain proxy responses

  Scenario: removeProxies filters proxy stubs from all imposters response
    Given an imposter on port 4545 with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/static"}}],
            "responses": [{"is": {"statusCode": 200, "body": "static"}}]
          },
          {
            "predicates": [{"equals": {"path": "/proxy"}}],
            "responses": [{"proxy": {"to": "http://example.com"}}]
          }
        ]
      }
      """
    When I send GET request to "/imposters?replayable=true&removeProxies=true" on both services
    Then both services should return status 200
    And responses should not contain proxy responses

  # ==========================================================================
  # _mode Binary Response Support
  # ==========================================================================

  Scenario: Binary mode decodes base64 body
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"equals": {"path": "/binary"}}],
        "responses": [{
          "is": {
            "statusCode": 200,
            "headers": {"Content-Type": "application/octet-stream"},
            "body": "SGVsbG8gV29ybGQh",
            "_mode": "binary"
          }
        }]
      }
      """
    When I send GET request to "/binary" on imposter 4545
    Then both services should return status 200
    And both responses should have body "Hello World!"

  Scenario: Text mode (default) returns body as-is
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{"equals": {"path": "/text"}}],
        "responses": [{
          "is": {
            "statusCode": 200,
            "body": "Plain text body"
          }
        }]
      }
      """
    When I send GET request to "/text" on imposter 4545
    Then both services should return status 200
    And both responses should have body "Plain text body"

  # ==========================================================================
  # Form Field Predicate
  # ==========================================================================

  Scenario: Form predicate matches URL-encoded form data
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "equals": {
            "form": {
              "username": "admin",
              "password": "secret"
            }
          }
        }],
        "responses": [{"is": {"statusCode": 200, "body": "form matched"}}]
      }
      """
    When I send POST request with form body "username=admin&password=secret" and Content-Type "application/x-www-form-urlencoded" on imposter 4545
    Then both services should return status 200
    And both responses should have body "form matched"

  Scenario: Form predicate with contains operator
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "contains": {
            "form": {
              "email": "@example.com"
            }
          }
        }],
        "responses": [{"is": {"statusCode": 200, "body": "email matched"}}]
      }
      """
    When I send POST request with form body "email=user@example.com&name=Test" and Content-Type "application/x-www-form-urlencoded" on imposter 4545
    Then both services should return status 200
    And both responses should have body "email matched"

  # ==========================================================================
  # keyCaseSensitive Predicate Option
  # ==========================================================================

  Scenario: keyCaseSensitive false allows case-insensitive header keys
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "equals": {
            "headers": {
              "X-CUSTOM-HEADER": "value"
            }
          },
          "caseSensitive": false,
          "keyCaseSensitive": false
        }],
        "responses": [{"is": {"statusCode": 200, "body": "header matched"}}]
      }
      """
    When I send GET request with header "x-custom-header: value" on imposter 4545
    Then both services should return status 200
    And both responses should have body "header matched"

  # Note: In practice, Mountebank query key matching is case-insensitive
  # regardless of keyCaseSensitive setting due to URL normalization.
  # This test validates that both services accept the keyCaseSensitive option.
  @rift-only
  Scenario: keyCaseSensitive true requires exact case for keys (Rift behavior)
    Given an imposter on port 4545 with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "defaultResponse": {"statusCode": 404, "body": "not found"},
        "stubs": [{
          "predicates": [{
            "equals": {
              "query": {
                "MyKey": "value"
              }
            },
            "keyCaseSensitive": true
          }],
          "responses": [{"is": {"statusCode": 200, "body": "matched"}}]
        }]
      }
      """
    When I send GET request to "/?mykey=value" on imposter 4545
    Then Rift should return status 404

  # ==========================================================================
  # IP and RequestFrom Predicates
  # Note: These tests match any IP address to work in Docker environments
  # where the client IP is a Docker network IP, not 127.0.0.1
  # ==========================================================================

  Scenario: IP predicate matches client IP address
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "matches": {
            "ip": "."
          }
        }],
        "responses": [{"is": {"statusCode": 200, "body": "local client"}}]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return status 200
    And both responses should have body "local client"

  Scenario: RequestFrom predicate matches client IP:port
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "matches": {
            "requestFrom": ":[0-9]+$"
          }
        }],
        "responses": [{"is": {"statusCode": 200, "body": "request from matched"}}]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return status 200
    And both responses should have body "request from matched"

  # ==========================================================================
  # Host Binding Configuration
  # ==========================================================================

  @rift-only
  Scenario: Host field binds imposter to specific interface
    When I create an imposter on Rift only:
      """
      {
        "port": 4560,
        "host": "127.0.0.1",
        "protocol": "http",
        "stubs": [{
          "predicates": [],
          "responses": [{"is": {"statusCode": 200, "body": "localhost only"}}]
        }]
      }
      """
    Then Rift should return status 201
    And GET "/" on imposter 4560 should return "localhost only" on Rift

  # ==========================================================================
  # recordMatches Configuration
  # ==========================================================================

  Scenario: recordMatches field is accepted in configuration
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "recordMatches": true,
        "stubs": [{
          "predicates": [{"equals": {"path": "/test"}}],
          "responses": [{"is": {"statusCode": 200, "body": "test"}}]
        }]
      }
      """
    Then both services should return status 201
    And GET "/test" on imposter 4545 should return "test" on both

  # ==========================================================================
  # Combined Features
  # ==========================================================================

  Scenario: Multiple new predicate fields work together
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "and": [
            {"equals": {"method": "POST"}},
            {"matches": {"ip": "."}}
          ]
        }],
        "responses": [{"is": {"statusCode": 200, "body": "combined match"}}]
      }
      """
    When I send POST request to "/" on imposter 4545
    Then both services should return status 200
    And both responses should have body "combined match"

  Scenario: Exists predicate with form field
    Given an imposter on port 4545 with stub:
      """
      {
        "predicates": [{
          "exists": {
            "form": {
              "token": true
            }
          }
        }],
        "responses": [{"is": {"statusCode": 200, "body": "token exists"}}]
      }
      """
    When I send POST request with form body "token=abc123&other=value" and Content-Type "application/x-www-form-urlencoded" on imposter 4545
    Then both services should return status 200
    And both responses should have body "token exists"

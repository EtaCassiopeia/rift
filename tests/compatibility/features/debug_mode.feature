Feature: Debug Mode (Rift Extension)
  Tests for the X-Rift-Debug header that returns match information instead of executing responses.

  This is a Rift-only feature that helps diagnose stub matching issues.

  Background:
    Given Rift service is running
    And all imposters are cleared

  # ==========================================================================
  # Debug Mode with Matching Stub
  # ==========================================================================

  @rift-only
  Scenario: Debug mode returns match information for matching stub
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "name": "Test Service",
        "stubs": [
          {
            "id": "get-users",
            "predicates": [{"equals": {"method": "GET", "path": "/api/users"}}],
            "responses": [{"is": {"statusCode": 200, "body": "users list"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/api/users" with header "X-Rift-Debug: true"
    Then response should contain "debug"
    And response should contain "matched"
    And response should contain "stubIndex"
    And response should contain "get-users"

  @rift-only
  Scenario: Debug mode shows response preview
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/test"}}],
            "responses": [{"is": {"statusCode": 201, "body": "created"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/test" with header "X-Rift-Debug: true"
    Then response should contain "responsePreview"
    And response should contain "statusCode"
    And response should contain "201"

  # ==========================================================================
  # Debug Mode with No Match
  # ==========================================================================

  @rift-only
  Scenario: Debug mode shows all stubs when no match found
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "id": "stub-a",
            "predicates": [{"equals": {"path": "/a"}}],
            "responses": [{"is": {"body": "A"}}]
          },
          {
            "id": "stub-b",
            "predicates": [{"equals": {"path": "/b"}}],
            "responses": [{"is": {"body": "B"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/not-found" with header "X-Rift-Debug: true"
    Then response should contain "matched"
    And response should contain "false"
    And response should contain "allStubs"
    And response should contain "stub-a"
    And response should contain "stub-b"
    And response should contain "reason"

  # ==========================================================================
  # Debug Mode with Empty Imposter
  # ==========================================================================

  @rift-only
  Scenario: Debug mode shows reason when no stubs configured
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": []
      }
      """
    When I send GET request to "http://localhost:4545/any" with header "X-Rift-Debug: true"
    Then response should contain "matched"
    And response should contain "false"
    And response should contain "No stubs configured"

  # ==========================================================================
  # Debug Mode Does Not Execute Response
  # ==========================================================================

  @rift-only
  Scenario: Debug mode does not execute the actual response
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "recordRequests": true,
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/test"}}],
            "responses": [{"is": {"statusCode": 418, "body": "I'm a teapot"}}]
          }
        ]
      }
      """
    # With debug mode, the actual 418 response should not be returned
    When I send GET request to "http://localhost:4545/test" with header "X-Rift-Debug: true"
    Then response status should be 200
    And response should contain "debug"
    And response should contain "418"
    # The debug response itself should be JSON, not "I'm a teapot"
    And response should not contain "I'm a teapot"

  # ==========================================================================
  # Debug Mode Header Variations
  # ==========================================================================

  @rift-only
  Scenario: Debug mode accepts lowercase header
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"body": "catch-all"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/any" with header "x-rift-debug: true"
    Then response should contain "debug"

  @rift-only
  Scenario: Debug mode accepts value "1"
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"body": "catch-all"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/any" with header "X-Rift-Debug: 1"
    Then response should contain "debug"

  # ==========================================================================
  # Debug Mode Shows Request Details
  # ==========================================================================

  @rift-only
  Scenario: Debug mode shows request details
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"body": "catch-all"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/test/path?foo=bar" with header "X-Rift-Debug: true"
    Then response should contain "request"
    And response should contain "method"
    And response should contain "GET"
    And response should contain "path"
    And response should contain "/test/path"
    And response should contain "query"
    And response should contain "foo=bar"

  # ==========================================================================
  # Debug Mode Shows Imposter Info
  # ==========================================================================

  @rift-only
  Scenario: Debug mode shows imposter information
    Given I create an imposter on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "name": "My Test Service",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"body": "ok"}}]
          }
        ]
      }
      """
    When I send GET request to "http://localhost:4545/any" with header "X-Rift-Debug: true"
    Then response should contain "imposter"
    And response should contain "port"
    And response should contain "4545"
    And response should contain "name"
    And response should contain "My Test Service"
    And response should contain "stubCount"
    And response should contain "1"

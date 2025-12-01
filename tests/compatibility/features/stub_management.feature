Feature: Stub Management Compatibility
  Tests for stub management behavior, ordering, and edge cases.

  Both Mountebank and Rift use first-match-wins semantics for stub matching.
  Rift extends this with optional stub IDs and analysis warnings.

  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  # ==========================================================================
  # First-Match-Wins Behavior (Mountebank-compatible)
  # ==========================================================================

  Scenario: First matching stub is used when multiple stubs could match
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"startsWith": {"path": "/api"}}],
            "responses": [{"is": {"statusCode": 200, "body": "general api"}}]
          },
          {
            "predicates": [{"equals": {"path": "/api/users"}}],
            "responses": [{"is": {"statusCode": 200, "body": "specific users"}}]
          }
        ]
      }
      """
    Then both services should return status 201
    # The first stub (startsWith /api) matches first, so /api/users returns "general api"
    And GET "/api/users" on imposter 4545 should return "general api" on both

  Scenario: More specific stub matches when placed first
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/api/users"}}],
            "responses": [{"is": {"statusCode": 200, "body": "specific users"}}]
          },
          {
            "predicates": [{"startsWith": {"path": "/api"}}],
            "responses": [{"is": {"statusCode": 200, "body": "general api"}}]
          }
        ]
      }
      """
    Then both services should return status 201
    # The first stub (equals /api/users) matches exactly
    And GET "/api/users" on imposter 4545 should return "specific users" on both
    # Other /api paths match the second stub
    And GET "/api/orders" on imposter 4545 should return "general api" on both

  # ==========================================================================
  # Same Path Different Method (No Conflict)
  # ==========================================================================

  Scenario: Stubs with same path but different methods work independently
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/resource", "method": "GET"}}],
            "responses": [{"is": {"statusCode": 200, "body": "GET response"}}]
          },
          {
            "predicates": [{"equals": {"path": "/resource", "method": "POST"}}],
            "responses": [{"is": {"statusCode": 201, "body": "POST response"}}]
          },
          {
            "predicates": [{"equals": {"path": "/resource", "method": "DELETE"}}],
            "responses": [{"is": {"statusCode": 204, "body": ""}}]
          }
        ]
      }
      """
    Then both services should return status 201
    And GET "/resource" on imposter 4545 should return "GET response" on both
    And POST to "/resource" on imposter 4545 should return status 201 on both
    And DELETE "/resource" on imposter 4545 should return status 204 on both

  # ==========================================================================
  # Duplicate Predicates (Allowed but Second is Unreachable)
  # ==========================================================================

  Scenario: Duplicate predicates are allowed - first wins
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/test"}}],
            "responses": [{"is": {"statusCode": 200, "body": "first"}}]
          },
          {
            "predicates": [{"equals": {"path": "/test"}}],
            "responses": [{"is": {"statusCode": 200, "body": "second"}}]
          }
        ]
      }
      """
    Then both services should return status 201
    # First stub always matches, second is unreachable
    And GET "/test" on imposter 4545 should return "first" on both

  # ==========================================================================
  # Empty Predicates (Catch-All)
  # ==========================================================================

  Scenario: Empty predicates match all requests
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "catch all"}}]
          }
        ]
      }
      """
    Then both services should return status 201
    And GET "/any/path" on imposter 4545 should return "catch all" on both
    And GET "/another" on imposter 4545 should return "catch all" on both

  Scenario: Catch-all stub shadows all subsequent stubs
    When I create an imposter on both services:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "catch all"}}]
          },
          {
            "predicates": [{"equals": {"path": "/specific"}}],
            "responses": [{"is": {"statusCode": 200, "body": "specific"}}]
          }
        ]
      }
      """
    Then both services should return status 201
    # Catch-all matches first, specific stub is unreachable
    And GET "/specific" on imposter 4545 should return "catch all" on both

  # ==========================================================================
  # Stub Index Operations
  # ==========================================================================

  Scenario: Delete stub shifts subsequent indexes
    Given an imposter on port 4545 with stubs:
      """
      [
        {"predicates": [{"equals": {"path": "/a"}}], "responses": [{"is": {"statusCode": 200, "body": "A"}}]},
        {"predicates": [{"equals": {"path": "/b"}}], "responses": [{"is": {"statusCode": 200, "body": "B"}}]},
        {"predicates": [{"equals": {"path": "/c"}}], "responses": [{"is": {"statusCode": 200, "body": "C"}}]}
      ]
      """
    # Delete stub at index 0 (path /a)
    When I send DELETE request to "/imposters/4545/stubs/0" on both services
    Then both services should return status 200
    # /a stub is gone
    And GET "/a" on imposter 4545 should not return "A" on both
    # /b is now at index 0, /c is now at index 1
    And GET "/b" on imposter 4545 should return "B" on both
    And GET "/c" on imposter 4545 should return "C" on both

  Scenario: Add stub at specific index
    Given an imposter on port 4545 with stubs:
      """
      [
        {"predicates": [{"equals": {"path": "/first"}}], "responses": [{"is": {"statusCode": 200, "body": "first"}}]},
        {"predicates": [{"equals": {"path": "/last"}}], "responses": [{"is": {"statusCode": 200, "body": "last"}}]}
      ]
      """
    # Add stub at index 1 (between first and last)
    When I POST to "/imposters/4545/stubs" on both services:
      """
      {
        "index": 1,
        "stub": {
          "predicates": [{"equals": {"path": "/middle"}}],
          "responses": [{"is": {"statusCode": 200, "body": "middle"}}]
        }
      }
      """
    Then both services should return status 200
    And GET "/middle" on imposter 4545 should return "middle" on both

  # ==========================================================================
  # Rift Extensions (Rift-only behavior)
  # ==========================================================================

  @rift-only
  Scenario: Stub ID field is accepted and returned (Rift extension)
    When I POST to "/imposters" on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "id": "user-stub",
            "predicates": [{"equals": {"path": "/users"}}],
            "responses": [{"is": {"statusCode": 200, "body": "users"}}]
          }
        ]
      }
      """
    Then Rift should return status 201
    And Rift response should contain "user-stub"

  @rift-only
  Scenario: Rift returns warnings for shadowed stubs
    When I POST to "/imposters" on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "catch all"}}]
          },
          {
            "predicates": [{"equals": {"path": "/specific"}}],
            "responses": [{"is": {"statusCode": 200, "body": "specific"}}]
          }
        ]
      }
      """
    Then Rift should return status 201
    # Get imposter to check for warnings
    When I send GET request to "/imposters/4545" on Rift
    Then Rift response should contain "_rift"
    And Rift response should contain "warnings"
    And Rift response should contain "catch_all"

  @rift-only
  Scenario: Rift returns warnings for duplicate IDs
    When I POST to "/imposters" on Rift:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "id": "duplicate-id",
            "predicates": [{"equals": {"path": "/a"}}],
            "responses": [{"is": {"statusCode": 200, "body": "A"}}]
          },
          {
            "id": "duplicate-id",
            "predicates": [{"equals": {"path": "/b"}}],
            "responses": [{"is": {"statusCode": 200, "body": "B"}}]
          }
        ]
      }
      """
    Then Rift should return status 201
    When I send GET request to "/imposters/4545" on Rift
    Then Rift response should contain "duplicate_id"

  # ==========================================================================
  # Mountebank Behavioral Notes
  # ==========================================================================
  #
  # The following behaviors are documented for reference:
  #
  # 1. First-Match-Wins: Both Mountebank and Rift use first-match semantics.
  #    The first stub whose predicates match is used; subsequent matches are ignored.
  #
  # 2. No Overlap Detection: Mountebank does NOT warn about overlapping predicates.
  #    Rift provides optional warnings via the _rift.warnings field.
  #
  # 3. Index-Based Operations: Deleting a stub shifts all subsequent indexes.
  #    Clients must track index changes when managing stubs dynamically.
  #
  # 4. Empty Predicates: A stub with [] predicates matches ALL requests.
  #    Place catch-all stubs last to avoid shadowing specific stubs.
  #
  # 5. Stub IDs (Rift Extension): Rift supports an optional "id" field for stubs.
  #    This is ignored by Mountebank but useful for tracking stubs by name.

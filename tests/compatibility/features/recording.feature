Feature: Request Recording Compatibility
  Rift should record requests identically to Mountebank

  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  # ==========================================================================
  # Basic Recording
  # ==========================================================================

  Scenario: Record requests when enabled
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request to "/test1" on imposter 4545
    And I send POST request to "/test2" with body "data" on imposter 4545
    Then both services should have recorded 2 requests
    And recorded requests should match on both services

  Scenario: Do not record when disabled
    # Note: numberOfRequests is always incremented, but requests array is empty when disabled
    Given an imposter on port 4545 with recordRequests disabled
    When I send GET request to "/test" on imposter 4545
    Then imposter 4545 should have empty requests array on both services

  # ==========================================================================
  # Request Details Recording
  # ==========================================================================

  Scenario: Record request method
    Given an imposter on port 4545 with recordRequests enabled
    When I send DELETE request to "/resource" on imposter 4545
    Then recorded request should have method "DELETE" on both services

  Scenario: Record request path
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request to "/api/users/123" on imposter 4545
    Then recorded request should have path "/api/users/123" on both services

  Scenario: Record request headers
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request with headers on imposter 4545:
      | header        | value            |
      | X-Custom      | custom-value     |
      | Authorization | Bearer token123  |
    Then recorded request should have header "X-Custom" with value "custom-value" on both services

  Scenario: Record request body
    Given an imposter on port 4545 with recordRequests enabled
    When I send POST request to "/data" with body '{"key": "value"}' on imposter 4545
    Then recorded request should have body '{"key": "value"}' on both services

  Scenario: Record query parameters
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request to "/search?q=test&page=1" on imposter 4545
    Then recorded request should have query "q" with value "test" on both services
    And recorded request should have query "page" with value "1" on both services

  # ==========================================================================
  # Request Count
  # ==========================================================================

  Scenario: Track request count per imposter
    Given an imposter on port 4545 with recordRequests enabled
    When I send 5 GET requests to "/test" on imposter 4545
    Then imposter 4545 should have numberOfRequests equal to 5 on both services

  Scenario: Request count persists across different paths
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request to "/path1" on imposter 4545
    And I send GET request to "/path2" on imposter 4545
    And I send POST request to "/path3" on imposter 4545
    Then imposter 4545 should have numberOfRequests equal to 3 on both services

  # ==========================================================================
  # Clear Requests
  # ==========================================================================

  Scenario: Clear recorded requests
    Given an imposter on port 4545 with recordRequests enabled
    And I send 3 GET requests to "/test" on imposter 4545
    When I send DELETE request to "/imposters/4545/savedRequests" on both admin APIs
    Then both services should return status 200
    And imposter 4545 should have numberOfRequests equal to 0 on both services

  # ==========================================================================
  # Timestamp Recording
  # ==========================================================================

  Scenario: Record request timestamp
    Given an imposter on port 4545 with recordRequests enabled
    When I send GET request to "/timed" on imposter 4545
    Then recorded request should have timestamp on both services
    And timestamps should be within 5 seconds of each other

  # ==========================================================================
  # Multiple Imposters Recording
  # ==========================================================================

  Scenario: Record requests independently per imposter
    Given an imposter on port 4545 with recordRequests enabled
    And an imposter on port 4546 with recordRequests enabled
    When I send 2 GET requests to "/test" on imposter 4545
    And I send 3 GET requests to "/test" on imposter 4546
    Then imposter 4545 should have numberOfRequests equal to 2 on both services
    And imposter 4546 should have numberOfRequests equal to 3 on both services

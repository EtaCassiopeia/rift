Feature: Proxy Mode Compatibility
  Rift should handle proxy modes identically to Mountebank

  Background:
    Given both Mountebank and Rift services are running
    And all imposters are cleared

  # ==========================================================================
  # Proxy Response Type
  # ==========================================================================

  Scenario: Basic proxy forwards requests to backend
    Given a backend server running on port 4546 returning "backend response"
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {"to": "http://localhost:4546"}
          }]
        }]
      }
      """
    When I send GET request to "/api/test" on imposter 4545
    Then both services should return status 200
    And both responses should have body "backend response"

  # ==========================================================================
  # Proxy Modes
  # ==========================================================================

  Scenario: proxyOnce mode saves response and replays
    Given a backend server that tracks request count
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce"
            }
          }]
        }]
      }
      """
    When I send GET request to "/test" on imposter 4545
    And I send GET request to "/test" on imposter 4545
    Then backend should receive only 1 request on both services
    And both requests to imposter should return same response

  Scenario: proxyAlways mode always forwards to backend
    Given a backend server that tracks request count
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyAlways"
            }
          }]
        }]
      }
      """
    When I send GET request to "/test" on imposter 4545
    And I send GET request to "/test" on imposter 4545
    Then backend should receive 2 requests on both services

  Scenario: proxyTransparent mode does not save responses
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyTransparent"
            }
          }]
        }]
      }
      """
    When I send GET request to "/test" on imposter 4545
    Then imposter 4545 should have no saved responses on both services

  # ==========================================================================
  # Predicate Generators
  # ==========================================================================

  Scenario: predicateGenerators creates stub with path predicate
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"path": true}
              }]
            }
          }]
        }]
      }
      """
    When I send GET request to "/specific/path" on imposter 4545
    Then generated stub should have path predicate on both services

  Scenario: predicateGenerators with method and headers
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"method": true, "headers": {"Accept": true}}
              }]
            }
          }]
        }]
      }
      """
    When I send GET request with header "Accept: application/json" on imposter 4545
    Then generated stub should have method and header predicates on both services

  Scenario: predicateGenerators with caseSensitive option
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"path": true},
                "caseSensitive": false
              }]
            }
          }]
        }]
      }
      """
    When I send GET request to "/API/Test" on imposter 4545
    Then generated predicate should be case insensitive on both services

  Scenario: predicateGenerators with jsonpath
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"body": true},
                "jsonpath": {"selector": "$.user.id"}
              }]
            }
          }]
        }]
      }
      """
    When I send POST request with JSON body '{"user": {"id": 123}}' on imposter 4545
    Then generated stub should use jsonpath in predicate on both services

  Scenario: predicateGenerators with except
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"body": true},
                "except": "timestamp=[^&]*"
              }]
            }
          }]
        }]
      }
      """
    When I send POST request with body "data=test&timestamp=12345" on imposter 4545
    Then generated predicate should not include timestamp on both services

  Scenario: predicateGenerators with predicateOperator
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "predicateGenerators": [{
                "matches": {"path": true},
                "predicateOperator": "contains"
              }]
            }
          }]
        }]
      }
      """
    When I send GET request to "/api/users/123" on imposter 4545
    Then generated stub should use contains predicate on both services

  # ==========================================================================
  # Proxy Behaviors
  # ==========================================================================

  Scenario: addWaitBehavior captures response time
    Given a backend server with 100ms delay
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "addWaitBehavior": true
            }
          }]
        }]
      }
      """
    When I send GET request to "/slow" on imposter 4545
    Then generated stub should have wait behavior on both services

  Scenario: addDecorateBehavior modifies saved responses
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "mode": "proxyOnce",
              "addDecorateBehavior": "function(request, response) { response.headers['X-Proxied'] = 'true'; }"
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return status 200
    When I send GET request to "/" on imposter 4545
    Then response should have header "X-Proxied" on both services

  # ==========================================================================
  # Proxy Headers
  # ==========================================================================

  Scenario: injectHeaders adds headers to proxied request
    Given a backend server that echoes headers
    And an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {
              "to": "http://localhost:4546",
              "injectHeaders": {"X-Injected": "header-value"}
            }
          }]
        }]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then backend should receive X-Injected header on both services

  # ==========================================================================
  # Proxy with Stubs
  # ==========================================================================

  Scenario: Stubs take priority over proxy
    Given a backend server running on port 4546 returning "from backend"
    And an imposter on port 4545 with:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [
          {
            "predicates": [{"equals": {"path": "/local"}}],
            "responses": [{"is": {"statusCode": 200, "body": "from stub"}}]
          },
          {
            "responses": [{"proxy": {"to": "http://localhost:4546"}}]
          }
        ]
      }
      """
    When I send GET request to "/local" on imposter 4545
    Then both responses should have body "from stub"
    When I send GET request to "/proxied" on imposter 4545
    Then both responses should have body "from backend"

  # ==========================================================================
  # Proxy Error Handling
  # ==========================================================================

  Scenario: Proxy returns error when backend unavailable
    Given an imposter on port 4545 with proxy:
      """
      {
        "port": 4545,
        "protocol": "http",
        "stubs": [{
          "responses": [{
            "proxy": {"to": "http://localhost:9999"}
          }]
        }]
      }
      """
    When I send GET request to "/" on imposter 4545
    Then both services should return error status

  # ==========================================================================
  # Proxy with Recording
  # ==========================================================================

  Scenario: Proxy records requests when recordRequests enabled
    Given a backend server running on port 4546
    And an imposter on port 4545 with proxy and recordRequests:
      """
      {
        "port": 4545,
        "protocol": "http",
        "recordRequests": true,
        "stubs": [{
          "responses": [{
            "proxy": {"to": "http://localhost:4546"}
          }]
        }]
      }
      """
    When I send GET request to "/test" on imposter 4545
    Then both services should have recorded 1 request

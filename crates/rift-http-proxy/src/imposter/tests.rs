//! Tests for the imposter module.
//!
//! This module contains comprehensive tests for:
//! - Imposter configuration serialization/deserialization
//! - Predicate matching (all Mountebank predicates)
//! - Stub execution
//! - ImposterManager lifecycle

use super::*;
use std::collections::HashMap;

#[test]
fn test_imposter_config_default() {
    let json = r#"{"port": 8080}"#;
    let config: ImposterConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.port, Some(8080));
    assert_eq!(config.protocol, "http");
    assert!(!config.record_requests);
    assert!(config.stubs.is_empty());
}

#[test]
fn test_imposter_config_no_port() {
    // Port should be optional for auto-assignment
    let json = r#"{"protocol": "http"}"#;
    let config: ImposterConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.port, None);
    assert_eq!(config.protocol, "http");
}

#[test]
fn test_predicate_matching() {
    let stub = Stub {
        id: None,
        predicates: vec![serde_json::json!({
            "equals": {
                "method": "GET",
                "path": "/test"
            }
        })],
        responses: vec![StubResponse::Is {
            is: IsResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: Some(serde_json::json!({"message": "hello"})),
                ..Default::default()
            },
            behaviors: None,
            rift: None,
        }],
        scenario_name: None,
    };

    let empty_headers = HashMap::new();

    // Should match
    assert!(stub_matches(
        &stub.predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &stub.predicates,
        "get",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    )); // case-insensitive method

    // Should not match
    assert!(!stub_matches(
        &stub.predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &stub.predicates,
        "GET",
        "/other",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_execute_stub() {
    let config = ImposterConfig {
        port: Some(8080),
        protocol: "http".to_string(),
        name: Some("test".to_string()),
        record_requests: false,
        stubs: vec![],
        default_response: None,
        allow_cors: false,
        service_name: None,
        service_info: None,
        rift: None,
        ..Default::default()
    };
    let imposter = Imposter::new(config);

    let stub = Stub {
        id: None,
        predicates: vec![],
        responses: vec![StubResponse::Is {
            is: IsResponse {
                status_code: 201,
                headers: HashMap::new(),
                body: Some(serde_json::json!({"created": true})),
                ..Default::default()
            },
            behaviors: None,
            rift: None,
        }],
        scenario_name: None,
    };

    let result = imposter.execute_stub(&stub, 0);
    assert!(result.is_some());
    let (status, _headers, body, _behaviors, is_fault) = result.unwrap();
    assert_eq!(status, 201);
    assert!(body.contains("created"));
    assert!(!is_fault);
}

#[test]
fn test_parse_query_string() {
    let query = "name=alice&age=30";
    let parsed = parse_query_string(query);
    assert_eq!(parsed.get("name"), Some(&"alice".to_string()));
    assert_eq!(parsed.get("age"), Some(&"30".to_string()));
}

#[tokio::test]
async fn test_imposter_manager_create_delete() {
    let manager = ImposterManager::new();

    // Try to create an imposter on a high port (less likely to conflict)
    let config = ImposterConfig {
        port: Some(19999),
        protocol: "http".to_string(),
        name: Some("test".to_string()),
        record_requests: false,
        stubs: vec![],
        default_response: None,
        allow_cors: false,
        service_name: None,
        service_info: None,
        rift: None,
        ..Default::default()
    };

    // This may fail if port is in use, which is fine for testing
    let result = manager.create_imposter(config.clone()).await;
    if result.is_ok() {
        assert_eq!(manager.count(), 1);

        // Delete it
        let deleted = manager.delete_imposter(19999).await;
        assert!(deleted.is_ok());
        assert_eq!(manager.count(), 0);
    }
}

#[test]
fn test_add_decorate_behavior_serde() {
    let json = r#"{"to":"http://localhost:4546","mode":"proxyOnce","addDecorateBehavior":"function(request, response) { response.headers['X-Proxied'] = 'true'; }"}"#;

    // Test deserialization
    let proxy: ProxyResponse = serde_json::from_str(json).unwrap();
    assert!(proxy.add_decorate_behavior.is_some());
    assert_eq!(
        proxy.add_decorate_behavior.as_ref().unwrap(),
        "function(request, response) { response.headers['X-Proxied'] = 'true'; }"
    );

    // Test serialization - it should contain addDecorateBehavior
    let serialized = serde_json::to_string(&proxy).unwrap();
    println!("Serialized ProxyResponse: {serialized}");
    assert!(
        serialized.contains("addDecorateBehavior"),
        "Serialized JSON should contain addDecorateBehavior field"
    );
}

#[test]
fn test_imposter_config_with_add_decorate_behavior() {
    let json = r#"{"port": 4545, "protocol": "http", "stubs": [{"responses": [{"proxy": {"to": "http://localhost:4546", "mode": "proxyOnce", "addDecorateBehavior": "function(request, response) { response.headers['X-Proxied'] = 'true'; }"}}]}]}"#;

    // Test deserialization of full imposter config
    let config: ImposterConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.stubs.len(), 1);

    if let StubResponse::Proxy { proxy } = &config.stubs[0].responses[0] {
        println!("Deserialized proxy: {proxy:?}");
        assert!(
            proxy.add_decorate_behavior.is_some(),
            "add_decorate_behavior should be Some after deserialization"
        );
        assert_eq!(
            proxy.add_decorate_behavior.as_ref().unwrap(),
            "function(request, response) { response.headers['X-Proxied'] = 'true'; }"
        );
    } else {
        panic!("Expected Proxy response");
    }

    // Test serialization of full imposter config
    let serialized = serde_json::to_string_pretty(&config).unwrap();
    println!("Serialized ImposterConfig:\n{serialized}");
    assert!(
        serialized.contains("addDecorateBehavior"),
        "Serialized JSON should contain addDecorateBehavior field"
    );
}

#[test]
fn test_alternative_response_format_with_behaviors_array() {
    // Test format with: behaviors array (not _behaviors), statusCode as string, and proxy: null
    let json = r#"{
        "behaviors": [{"wait": 100}],
        "is": {
            "statusCode": "200",
            "headers": {"Content-Type": "application/json"},
            "body": "{\"message\": \"hello\"}"
        },
        "proxy": null
    }"#;

    let response: StubResponse = serde_json::from_str(json).unwrap();
    if let StubResponse::Is { is, behaviors, .. } = response {
        assert_eq!(is.status_code, 200);
        assert!(behaviors.is_some());
        let behaviors = behaviors.unwrap();
        assert_eq!(behaviors.get("wait").unwrap().as_u64(), Some(100));
    } else {
        panic!("Expected Is response");
    }
}

#[test]
fn test_status_code_as_string() {
    let json = r#"{
        "is": {
            "statusCode": "201",
            "headers": {},
            "body": null
        }
    }"#;

    let response: StubResponse = serde_json::from_str(json).unwrap();
    if let StubResponse::Is { is, .. } = response {
        assert_eq!(is.status_code, 201);
    } else {
        panic!("Expected Is response");
    }
}

#[test]
fn test_status_code_as_number() {
    let json = r#"{
        "is": {
            "statusCode": 404,
            "headers": {}
        }
    }"#;

    let response: StubResponse = serde_json::from_str(json).unwrap();
    if let StubResponse::Is { is, .. } = response {
        assert_eq!(is.status_code, 404);
    } else {
        panic!("Expected Is response");
    }
}

#[test]
fn test_behaviors_array_merged_to_object() {
    // Test that behaviors array format is converted to object
    let json = r#"{
        "behaviors": [
            {"wait": 50},
            {"decorate": "function() {}"}
        ],
        "is": {
            "statusCode": 200
        }
    }"#;

    let response: StubResponse = serde_json::from_str(json).unwrap();
    if let StubResponse::Is { behaviors, .. } = response {
        let behaviors = behaviors.expect("behaviors should be present");
        assert!(behaviors.get("wait").is_some());
        assert!(behaviors.get("decorate").is_some());
    } else {
        panic!("Expected Is response");
    }
}

#[test]
fn test_proxy_only_response() {
    // When only proxy is present (not null), it should parse as Proxy variant
    let json = r#"{
        "proxy": {
            "to": "http://example.com",
            "mode": "proxyTransparent"
        }
    }"#;

    let response: StubResponse = serde_json::from_str(json).unwrap();
    if let StubResponse::Proxy { proxy } = response {
        assert_eq!(proxy.to, "http://example.com");
        assert_eq!(proxy.mode, "proxyTransparent");
    } else {
        panic!("Expected Proxy response");
    }
}

#[test]
fn test_full_imposter_config_alternative_format() {
    // Test a complete imposter config with the alternative format
    let json = r#"{
        "port": 8201,
        "protocol": "http",
        "stubs": [
            {
                "predicates": [{"equals": {"method": "GET"}}],
                "responses": [
                    {
                        "behaviors": [{"wait": 0}],
                        "is": {
                            "statusCode": "200",
                            "headers": {"Content-Type": "application/json"},
                            "body": "{\"data\": \"test\"}"
                        },
                        "proxy": null
                    }
                ]
            }
        ]
    }"#;

    let config: ImposterConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.port, Some(8201));
    assert_eq!(config.stubs.len(), 1);
    assert_eq!(config.stubs[0].responses.len(), 1);

    if let StubResponse::Is { is, behaviors, .. } = &config.stubs[0].responses[0] {
        assert_eq!(is.status_code, 200);
        assert!(behaviors.is_some());
    } else {
        panic!("Expected Is response");
    }
}

// =============================================================================
// Comprehensive Predicate Tests (Mountebank Compatibility)
// =============================================================================

#[test]
fn test_predicate_ends_with() {
    let predicates = vec![serde_json::json!({
        "endsWith": {"path": "-details"}
    })];

    let empty_headers = HashMap::new();

    // Should match
    assert!(stub_matches(
        &predicates,
        "GET",
        "/api/lender-details",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "GET",
        "/user-details",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));

    // Should not match
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/details/other",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/api/details/v1",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_deep_equals_method() {
    let predicates = vec![serde_json::json!({
        "deepEquals": {"method": "GET"}
    })];

    let empty_headers = HashMap::new();

    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "get",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    )); // case-insensitive
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_deep_equals_body() {
    let predicates = vec![serde_json::json!({
        "deepEquals": {"body": ""}
    })];

    let empty_headers = HashMap::new();

    // Empty body should match
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        Some(""),
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));

    // Non-empty body should not match
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        Some("content"),
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_contains_query() {
    let predicates = vec![serde_json::json!({
        "contains": {"query": {"lenderIds": "CofTest"}}
    })];

    let empty_headers = HashMap::new();

    // Should match - query contains "CofTest"
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        Some("lenderIds=CofTestWL"),
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        Some("lenderIds=CofTest"),
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        Some("lenderIds=123CofTest456"),
        &empty_headers,
        None,
        None,
        None,
        None
    ));

    // Should not match
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/test",
        Some("lenderIds=Other"),
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_equals_headers() {
    let predicates = vec![serde_json::json!({
        "equals": {"headers": {"Content-Type": "application/json"}}
    })];

    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &headers,
        None,
        None,
        None,
        None
    ));

    // Header key lookup is case-insensitive
    let mut headers_lower = HashMap::new();
    headers_lower.insert("content-type".to_string(), "application/json".to_string());
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &headers_lower,
        None,
        None,
        None,
        None
    ));

    // Wrong value
    let mut wrong_headers = HashMap::new();
    wrong_headers.insert("Content-Type".to_string(), "text/html".to_string());
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &wrong_headers,
        None,
        None,
        None,
        None
    ));

    // Missing header
    let empty_headers = HashMap::new();
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_exists() {
    let predicates = vec![serde_json::json!({
        "exists": {
            "query": {"token": true},
            "headers": {"Authorization": true},
            "body": true
        }
    })];

    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer xyz".to_string());

    // All exist
    assert!(stub_matches(
        &predicates,
        "POST",
        "/test",
        Some("token=abc"),
        &headers,
        Some("body content"),
        None,
        None,
        None
    ));

    // Missing query param
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &headers,
        Some("body content"),
        None,
        None,
        None
    ));

    // Missing header
    let empty_headers = HashMap::new();
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        Some("token=abc"),
        &empty_headers,
        Some("body content"),
        None,
        None,
        None
    ));

    // Missing body
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        Some("token=abc"),
        &headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_logical_not() {
    let predicates = vec![serde_json::json!({
        "not": {"equals": {"method": "DELETE"}}
    })];

    let empty_headers = HashMap::new();

    // Should match anything except DELETE
    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "DELETE",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_logical_or() {
    let predicates = vec![serde_json::json!({
        "or": [
            {"equals": {"method": "GET"}},
            {"equals": {"method": "HEAD"}}
        ]
    })];

    let empty_headers = HashMap::new();

    assert!(stub_matches(
        &predicates,
        "GET",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "HEAD",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_logical_and() {
    let predicates = vec![serde_json::json!({
        "and": [
            {"equals": {"method": "GET"}},
            {"startsWith": {"path": "/api"}}
        ]
    })];

    let empty_headers = HashMap::new();

    assert!(stub_matches(
        &predicates,
        "GET",
        "/api/users",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/api/users",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/other",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_matches_regex_all_fields() {
    let predicates = vec![serde_json::json!({
        "matches": {
            "path": "^/api/v[0-9]+/",
            "method": "^(GET|POST)$"
        }
    })];

    let empty_headers = HashMap::new();

    assert!(stub_matches(
        &predicates,
        "GET",
        "/api/v1/users",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(stub_matches(
        &predicates,
        "POST",
        "/api/v2/items",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "DELETE",
        "/api/v1/users",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "GET",
        "/other/path",
        None,
        &empty_headers,
        None,
        None,
        None,
        None
    ));
}

#[test]
fn test_predicate_matches_body_regex() {
    let predicates = vec![serde_json::json!({
        "matches": {"body": "\"userId\":\\s*\"[a-f0-9-]+\""}
    })];

    let empty_headers = HashMap::new();

    assert!(stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        Some(r#"{"userId": "abc-123-def"}"#),
        None,
        None,
        None
    ));
    assert!(!stub_matches(
        &predicates,
        "POST",
        "/test",
        None,
        &empty_headers,
        Some(r#"{"userId": "invalid!"}"#),
        None,
        None,
        None
    ));
}

//! Stub generation from recorded requests/responses.

use super::types::{RecordedResponse, RequestSignature};
use std::collections::HashMap;

/// Generate a Mountebank-compatible stub from a recorded request/response
// Public API for predicate generator export
pub fn generate_stub(
    signature: &RequestSignature,
    response: &RecordedResponse,
    include_method: bool,
    include_path: bool,
    include_query: bool,
    include_headers: &[String],
) -> serde_json::Value {
    let mut predicates = serde_json::Map::new();

    if include_method {
        predicates.insert(
            "method".to_string(),
            serde_json::json!({ "equals": signature.method }),
        );
    }

    if include_path {
        predicates.insert(
            "path".to_string(),
            serde_json::json!({ "equals": signature.path }),
        );
    }

    if include_query {
        if let Some(ref query) = signature.query {
            // Parse query string into map
            let query_map: HashMap<String, String> = query
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    Some((parts.next()?.to_string(), parts.next()?.to_string()))
                })
                .collect();
            if !query_map.is_empty() {
                predicates.insert(
                    "query".to_string(),
                    serde_json::json!({ "equals": query_map }),
                );
            }
        }
    }

    if !include_headers.is_empty() {
        let header_predicates: HashMap<String, String> = signature
            .headers
            .iter()
            .filter(|(k, _)| include_headers.iter().any(|h| h.eq_ignore_ascii_case(k)))
            .cloned()
            .collect();
        if !header_predicates.is_empty() {
            predicates.insert(
                "headers".to_string(),
                serde_json::json!({ "equals": header_predicates }),
            );
        }
    }

    // Build response
    let body_str = String::from_utf8_lossy(&response.body).to_string();
    let mut response_obj = serde_json::json!({
        "statusCode": response.status,
        "headers": response.headers,
        "body": body_str,
    });

    // Add wait behavior if latency was captured
    if let Some(latency) = response.latency_ms {
        response_obj["_behaviors"] = serde_json::json!({
            "wait": latency
        });
    }

    serde_json::json!({
        "predicates": [{ "and": predicates }],
        "responses": [{ "is": response_obj }]
    })
}

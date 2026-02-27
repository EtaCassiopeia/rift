//! Stub generation from recorded requests/responses.

use super::types::{RecordedResponse, RequestSignature};
use crate::imposter::parse_query_string;
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
            let query_map = parse_query_string(query);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::types::{RecordedResponse, RequestSignature};

    fn make_response() -> RecordedResponse {
        RecordedResponse {
            status: 200,
            headers: HashMap::new(),
            body: b"OK".to_vec(),
            latency_ms: None,
            timestamp_secs: 0,
        }
    }

    // Fix #96: Query params are now URL-decoded via parse_query_string
    #[test]
    fn test_stub_generator_url_decodes_query_params() {
        let sig = RequestSignature::new("GET", "/search", Some("name=John%20Doe"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        assert_eq!(
            query_equals["name"].as_str().unwrap(),
            "John Doe",
            "Query param values should be URL-decoded in generated predicates"
        );
    }

    // Fix #97: Bare params (no =) are now preserved via parse_query_string
    #[test]
    fn test_stub_generator_preserves_bare_query_params() {
        let sig = RequestSignature::new("GET", "/test", Some("flag&key=value"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        assert_eq!(
            query_equals["flag"].as_str().unwrap(),
            "",
            "Bare query params should be present with empty value"
        );
        assert_eq!(query_equals["key"].as_str().unwrap(), "value");
    }

    // Fix #98: Multi-valued params are now comma-joined via parse_query_string
    #[test]
    fn test_stub_generator_comma_joins_multi_valued_params() {
        let sig = RequestSignature::new("GET", "/test", Some("color=red&color=blue"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        assert_eq!(
            query_equals["color"].as_str().unwrap(),
            "red,blue",
            "Multi-valued query params should be comma-joined"
        );
    }
}

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

    // =========================================================================
    // Bug B: Query params not URL-decoded in generated predicates
    // generate_stub parses query params by splitting on '&' and '=' but does
    // NOT URL-decode the values. The predicate matcher (parse_query_string)
    // DOES URL-decode, so encoded values like "John%20Doe" will never match
    // the decoded "John Doe" in the matcher.
    // =========================================================================

    #[test]
    fn test_stub_generator_url_decodes_query_params() {
        // Query: name=John%20Doe
        // CORRECT: generated predicate should have "John Doe" (decoded)
        // BUG: generated predicate has "John%20Doe" (raw/encoded)
        let sig = RequestSignature::new("GET", "/search", Some("name=John%20Doe"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        // BUG: Value is "John%20Doe" (not decoded) instead of "John Doe".
        // When this predicate is later matched, parse_query_string decodes
        // the incoming query to "John Doe", which won't match "John%20Doe".
        assert_eq!(
            query_equals["name"].as_str().unwrap(),
            "John%20Doe",
            "BUG(B): Query param values are not URL-decoded in generated predicates; \
             expected 'John Doe' (decoded), got 'John%20Doe'"
        );
    }

    // =========================================================================
    // Bug C: Bare query params (`?flag`) silently dropped (same as #84)
    // generate_stub uses `parts.next()?` for the value part, which returns
    // None for bare params (no '=' sign), causing filter_map to drop them.
    // This is the same bug pattern fixed for predicate matching in Issue #84.
    // =========================================================================

    #[test]
    fn test_stub_generator_preserves_bare_query_params() {
        // Query: flag&key=value
        // CORRECT: generated predicate should have {"flag": "", "key": "value"}
        // BUG: "flag" is silently dropped because parts.next()? returns None
        let sig = RequestSignature::new("GET", "/test", Some("flag&key=value"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        // BUG: "flag" is missing from the generated predicate.
        // Only "key" is present because bare params without '=' are dropped.
        assert!(
            query_equals.get("flag").is_none(),
            "BUG(C): Bare query params are silently dropped in stub generation; \
             expected 'flag' to be present with empty value, but it is missing"
        );
    }

    // =========================================================================
    // Bug D: Multi-valued query params overwritten (same as #83)
    // generate_stub uses .collect() into HashMap, which overwrites duplicate
    // keys with the last value. This is the same bug pattern fixed for
    // predicate matching in Issue #83.
    // =========================================================================

    #[test]
    fn test_stub_generator_comma_joins_multi_valued_params() {
        // Query: color=red&color=blue
        // CORRECT: generated predicate should have {"color": "red,blue"} (comma-joined)
        // BUG: .collect() overwrites to {"color": "blue"} (last wins)
        let sig = RequestSignature::new("GET", "/test", Some("color=red&color=blue"), &[]);
        let resp = make_response();

        let stub = generate_stub(&sig, &resp, false, false, true, &[]);
        let query_equals = &stub["predicates"][0]["and"]["query"]["equals"];

        // BUG: Value is "blue" (last wins) instead of "red,blue" (comma-joined).
        // The predicate matcher (parse_query_string) comma-joins duplicate keys,
        // producing "red,blue", which won't match the generated "blue".
        assert_eq!(
            query_equals["color"].as_str().unwrap(),
            "blue",
            "BUG(D): Multi-valued query params are overwritten in stub generation; \
             expected 'red,blue' (comma-joined like predicate matcher), got 'blue'"
        );
    }
}

//! Predicate matching logic for Mountebank-compatible stub matching.
//!
//! Supports: equals, deepEquals, contains, startsWith, endsWith, matches, exists, not, or, and
//! Also supports requestFrom, ip, and form fields.

use crate::behaviors::{extract_jsonpath, extract_xpath};
use crate::imposter::types::{Predicate, PredicateOperation, PredicateSelector};
use std::collections::HashMap;
use tracing::warn;

/// Check if a stub matches a request based on its predicates
#[allow(clippy::too_many_arguments)]
pub fn stub_matches(
    predicates: &[Predicate],
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    request_from: Option<&str>,
    client_ip: Option<&str>,
    form: Option<&HashMap<String, String>>,
) -> bool {
    // If no predicates, match everything
    if predicates.is_empty() {
        return true;
    }

    // All predicates must match (implicit AND)
    for predicate in predicates {
        if !predicate_matches(
            predicate,
            method,
            path,
            query,
            headers,
            body,
            request_from,
            client_ip,
            form,
        ) {
            return false;
        }
    }
    true
}

/// Parse query string for predicate matching, URL-decoding both keys and values
pub fn parse_query(query: Option<&str>) -> HashMap<String, String> {
    query.map_or_else(HashMap::new, parse_query_string)
}

/// Check if a single predicate matches (Mountebank-compatible)
/// Supports: equals, deepEquals, contains, startsWith, endsWith, matches, exists, not, or, and
/// Also supports requestFrom, ip, and form fields
#[allow(clippy::too_many_arguments)]
pub fn predicate_matches(
    predicate: &Predicate,
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    request_from: Option<&str>,
    client_ip: Option<&str>,
    form: Option<&HashMap<String, String>>,
) -> bool {
    // Get predicate options
    let case_sensitive = predicate.parameters.case_sensitive.unwrap_or(false);

    // Get keyCaseSensitive option (defaults to caseSensitive value if not specified)
    let key_case_sensitive = predicate
        .parameters
        .key_case_sensitive
        .unwrap_or(case_sensitive);

    let except_pattern = Some(predicate.parameters.except.as_str()).filter(|s| !s.is_empty());

    // Helper to apply except pattern
    let apply_except = |value: &str| -> String {
        if let Some(pattern) = except_pattern {
            if let Ok(re) = regex::Regex::new(pattern) {
                return re.replace_all(value, "").to_string();
            }
        }
        value.to_string()
    };

    // Helper for string comparison with case sensitivity
    let str_equals = |expected: &str, actual: &str| -> bool {
        if case_sensitive {
            expected == actual
        } else {
            expected.eq_ignore_ascii_case(actual)
        }
    };

    let str_contains = |haystack: &str, needle: &str| -> bool {
        if case_sensitive {
            haystack.contains(needle)
        } else {
            haystack.to_lowercase().contains(&needle.to_lowercase())
        }
    };

    let str_starts_with = |haystack: &str, needle: &str| -> bool {
        if case_sensitive {
            haystack.starts_with(needle)
        } else {
            haystack.to_lowercase().starts_with(&needle.to_lowercase())
        }
    };

    let str_ends_with = |haystack: &str, needle: &str| -> bool {
        if case_sensitive {
            haystack.ends_with(needle)
        } else {
            haystack.to_lowercase().ends_with(&needle.to_lowercase())
        }
    };

    // Build request context for field access
    let query_map = parse_query(query);
    let body_str = body.unwrap_or("");

    // Handle jsonpath parameter - extract value from JSON body
    let extracted_body: String;
    let effective_body = match &predicate.parameters.selector {
        Some(PredicateSelector::JsonPath { selector }) => {
            extracted_body = extract_jsonpath(body_str, selector).unwrap_or_default();
            &extracted_body
        }
        Some(PredicateSelector::XPath {
            selector,
            namespaces,
        }) => {
            extracted_body = extract_xpath(body_str, selector).unwrap_or_default();
            if namespaces.is_some() {
                warn!("XPath namespaces are not supported yet");
            }
            &extracted_body
        }
        None => body_str,
    };

    match &predicate.operation {
        PredicateOperation::Equals(fields) => {
            check_predicate_fields(
                fields,
                method,
                path,
                &query_map,
                headers,
                effective_body,
                &apply_except,
                str_equals,
                false, // not deep equals
                request_from,
                client_ip,
                form,
                key_case_sensitive,
            )
        }
        PredicateOperation::DeepEquals(fields) => {
            check_predicate_fields(
                fields,
                method,
                path,
                &query_map,
                headers,
                effective_body,
                &apply_except,
                str_equals,
                true, // deep equals
                request_from,
                client_ip,
                form,
                key_case_sensitive,
            )
        }
        PredicateOperation::Contains(fields) => check_predicate_fields(
            fields,
            method,
            path,
            &query_map,
            headers,
            effective_body,
            &apply_except,
            |expected, actual| str_contains(actual, expected),
            false,
            request_from,
            client_ip,
            form,
            key_case_sensitive,
        ),
        PredicateOperation::StartsWith(fields) => check_predicate_fields(
            fields,
            method,
            path,
            &query_map,
            headers,
            effective_body,
            &apply_except,
            |expected, actual| str_starts_with(actual, expected),
            false,
            request_from,
            client_ip,
            form,
            key_case_sensitive,
        ),
        PredicateOperation::EndsWith(fields) => check_predicate_fields(
            fields,
            method,
            path,
            &query_map,
            headers,
            effective_body,
            &apply_except,
            |expected, actual| str_ends_with(actual, expected),
            false,
            request_from,
            client_ip,
            form,
            key_case_sensitive,
        ),
        PredicateOperation::Matches(fields) => check_predicate_fields_regex(
            fields,
            method,
            path,
            &query_map,
            headers,
            effective_body,
            &apply_except,
            case_sensitive,
            request_from,
            client_ip,
            form,
            key_case_sensitive,
        ),
        PredicateOperation::Exists(fields) => {
            check_exists_predicate(fields, &query_map, headers, effective_body, form)
        }
        PredicateOperation::Not(inner) => !predicate_matches(
            inner,
            method,
            path,
            query,
            headers,
            body,
            request_from,
            client_ip,
            form,
        ),
        PredicateOperation::Or(children) => children.iter().any(|p| {
            predicate_matches(
                p,
                method,
                path,
                query,
                headers,
                body,
                request_from,
                client_ip,
                form,
            )
        }),
        PredicateOperation::And(children) => children.iter().all(|p| {
            predicate_matches(
                p,
                method,
                path,
                query,
                headers,
                body,
                request_from,
                client_ip,
                form,
            )
        }),
    }
}

/// Check predicate fields against request values
/// Supports: method, path, body, query, headers, requestFrom, ip, form
#[allow(clippy::too_many_arguments)]
fn check_predicate_fields<F>(
    obj: &HashMap<String, serde_json::Value>,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &str,
    apply_except: &impl Fn(&str) -> String,
    compare: F,
    deep_equals: bool,
    request_from: Option<&str>,
    client_ip: Option<&str>,
    form: Option<&HashMap<String, String>>,
    key_case_sensitive: bool,
) -> bool
where
    F: Fn(&str, &str) -> bool,
{
    // Helper for key comparison based on keyCaseSensitive
    let key_matches = |expected_key: &str, actual_key: &str| -> bool {
        if key_case_sensitive {
            expected_key == actual_key
        } else {
            expected_key.eq_ignore_ascii_case(actual_key)
        }
    };

    // Check method
    if let Some(expected) = obj.get("method").and_then(|v| v.as_str()) {
        if !compare(expected, method) {
            return false;
        }
    }

    // Check path
    if let Some(expected) = obj.get("path").and_then(|v| v.as_str()) {
        let actual = apply_except(path);
        if !compare(expected, &actual) {
            return false;
        }
    }

    // Check body
    if let Some(expected) = obj.get("body") {
        let expected_str = match expected {
            serde_json::Value::String(s) => s.clone(),
            _ => expected.to_string(),
        };
        let actual = apply_except(body);
        if !compare(&expected_str, &actual) {
            return false;
        }
    }

    // Check requestFrom (IP:port) - Mountebank compatible
    if let Some(expected) = obj.get("requestFrom").and_then(|v| v.as_str()) {
        let actual = request_from.unwrap_or("");
        let actual = apply_except(actual);
        if !compare(expected, &actual) {
            return false;
        }
    }

    // Check ip (just the IP address) - Mountebank compatible
    if let Some(expected) = obj.get("ip").and_then(|v| v.as_str()) {
        let actual = client_ip.unwrap_or("");
        let actual = apply_except(actual);
        if !compare(expected, &actual) {
            return false;
        }
    }

    // Check form fields (parsed from application/x-www-form-urlencoded) - Mountebank compatible
    if let Some(expected_form) = obj.get("form") {
        if let Some(expected_obj) = expected_form.as_object() {
            let actual_form = form.cloned().unwrap_or_default();

            // For deepEquals, check exact match (same number of fields)
            if deep_equals && expected_obj.len() != actual_form.len() {
                return false;
            }

            for (key, expected_val) in expected_obj {
                let expected_str = match expected_val {
                    serde_json::Value::String(s) => s.clone(),
                    _ => expected_val.to_string(),
                };
                // Find key using keyCaseSensitive option
                let actual = actual_form
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !compare(&expected_str, &actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    // Check query parameters
    if let Some(expected_query) = obj.get("query") {
        if let Some(expected_obj) = expected_query.as_object() {
            // For deepEquals, check exact match (same number of params)
            if deep_equals && expected_obj.len() != query.len() {
                return false;
            }

            for (key, expected_val) in expected_obj {
                let expected_str = match expected_val {
                    serde_json::Value::String(s) => s.clone(),
                    _ => expected_val.to_string(),
                };
                // Find key using keyCaseSensitive option
                let actual = query
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !compare(&expected_str, &actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    // Check headers
    if let Some(expected_headers) = obj.get("headers") {
        if let Some(expected_obj) = expected_headers.as_object() {
            // For deepEquals, check exact match
            if deep_equals && expected_obj.len() != headers.len() {
                return false;
            }

            for (key, expected_val) in expected_obj {
                let expected_str = match expected_val {
                    serde_json::Value::String(s) => s.clone(),
                    _ => expected_val.to_string(),
                };
                // Headers use keyCaseSensitive option
                let actual = headers
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !compare(&expected_str, &actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    true
}

/// Check predicate fields with regex matching
/// Supports: method, path, body, query, headers, requestFrom, ip, form
#[allow(clippy::too_many_arguments)]
fn check_predicate_fields_regex(
    obj: &HashMap<String, serde_json::Value>,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &str,
    apply_except: &impl Fn(&str) -> String,
    case_sensitive: bool,
    request_from: Option<&str>,
    client_ip: Option<&str>,
    form: Option<&HashMap<String, String>>,
    key_case_sensitive: bool,
) -> bool {
    let build_regex = |pattern: &str| -> Option<regex::Regex> {
        if case_sensitive {
            regex::Regex::new(pattern).ok()
        } else {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .ok()
        }
    };

    // Helper for key comparison based on keyCaseSensitive
    let key_matches = |expected_key: &str, actual_key: &str| -> bool {
        if key_case_sensitive {
            expected_key == actual_key
        } else {
            expected_key.eq_ignore_ascii_case(actual_key)
        }
    };

    // Check method
    if let Some(pattern) = obj.get("method").and_then(|v| v.as_str()) {
        if let Some(re) = build_regex(pattern) {
            if !re.is_match(method) {
                return false;
            }
        }
    }

    // Check path
    if let Some(pattern) = obj.get("path").and_then(|v| v.as_str()) {
        if let Some(re) = build_regex(pattern) {
            let actual = apply_except(path);
            if !re.is_match(&actual) {
                return false;
            }
        }
    }

    // Check body
    if let Some(pattern) = obj.get("body").and_then(|v| v.as_str()) {
        if let Some(re) = build_regex(pattern) {
            let actual = apply_except(body);
            if !re.is_match(&actual) {
                return false;
            }
        }
    }

    // Check requestFrom
    if let Some(pattern) = obj.get("requestFrom").and_then(|v| v.as_str()) {
        if let Some(re) = build_regex(pattern) {
            let actual = apply_except(request_from.unwrap_or(""));
            if !re.is_match(&actual) {
                return false;
            }
        }
    }

    // Check ip
    if let Some(pattern) = obj.get("ip").and_then(|v| v.as_str()) {
        if let Some(re) = build_regex(pattern) {
            let actual = apply_except(client_ip.unwrap_or(""));
            if !re.is_match(&actual) {
                return false;
            }
        }
    }

    // Check form fields
    if let Some(expected_form) = obj.get("form").and_then(|v| v.as_object()) {
        let actual_form = form.cloned().unwrap_or_default();
        for (key, pattern_val) in expected_form {
            let pattern = match pattern_val {
                serde_json::Value::String(s) => s.as_str(),
                _ => continue,
            };
            if let Some(re) = build_regex(pattern) {
                let actual = actual_form
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !re.is_match(&actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    // Check query parameters
    if let Some(expected_query) = obj.get("query").and_then(|v| v.as_object()) {
        for (key, pattern_val) in expected_query {
            let pattern = match pattern_val {
                serde_json::Value::String(s) => s.as_str(),
                _ => continue,
            };
            if let Some(re) = build_regex(pattern) {
                let actual = query
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !re.is_match(&actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    // Check headers
    if let Some(expected_headers) = obj.get("headers").and_then(|v| v.as_object()) {
        for (key, pattern_val) in expected_headers {
            let pattern = match pattern_val {
                serde_json::Value::String(s) => s.as_str(),
                _ => continue,
            };
            if let Some(re) = build_regex(pattern) {
                let actual = headers
                    .iter()
                    .find(|(k, _)| key_matches(key, k))
                    .map(|(_, v)| v.as_str());

                match actual {
                    Some(actual) => {
                        let actual = apply_except(actual);
                        if !re.is_match(&actual) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
    }

    true
}

/// Check exists predicate - verifies field presence or absence
/// Supports: body, query, headers, form
fn check_exists_predicate(
    obj: &HashMap<String, serde_json::Value>,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &str,
    form: Option<&HashMap<String, String>>,
) -> bool {
    // Check body exists
    if let Some(should_exist) = obj.get("body").and_then(|v| v.as_bool()) {
        let exists = !body.is_empty();
        if exists != should_exist {
            return false;
        }
    }

    // Check query parameters exist
    if let Some(expected_query) = obj.get("query").and_then(|v| v.as_object()) {
        for (key, should_exist_val) in expected_query {
            let should_exist = should_exist_val.as_bool().unwrap_or(true);
            let exists = query.contains_key(key);
            if exists != should_exist {
                return false;
            }
        }
    }

    // Check headers exist
    if let Some(expected_headers) = obj.get("headers").and_then(|v| v.as_object()) {
        for (key, should_exist_val) in expected_headers {
            let should_exist = should_exist_val.as_bool().unwrap_or(true);
            let exists = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(key));
            if exists != should_exist {
                return false;
            }
        }
    }

    // Check form fields exist
    if let Some(expected_form) = obj.get("form").and_then(|v| v.as_object()) {
        let actual_form = form.cloned().unwrap_or_default();
        for (key, should_exist_val) in expected_form {
            let should_exist = should_exist_val.as_bool().unwrap_or(true);
            let exists = actual_form.contains_key(key);
            if exists != should_exist {
                return false;
            }
        }
    }

    true
}

/// Parse query string into HashMap (public helper)
/// URL-decodes both keys and values to properly handle encoded characters
pub fn parse_query_string(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            // URL-decode both key and value to handle encoded characters like %2C -> ,
            let decoded_key = urlencoding::decode(key).unwrap_or_default().into_owned();
            let decoded_value = urlencoding::decode(value).unwrap_or_default().into_owned();
            Some((decoded_key, decoded_value))
        })
        .collect()
}

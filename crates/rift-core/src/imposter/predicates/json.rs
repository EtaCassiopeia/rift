//! JSON-shaped predicate helpers: value stringification, recursive `exists` checks,
//! and recursive JSON comparison used by the `equals`/`deepEquals`/`matches` operators.

use std::collections::HashMap;

/// Convert a JSON value to its string representation for predicate comparison.
/// Strings are unwrapped (no quotes), other primitives use their natural representation.
fn json_value_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => val.to_string(),
    }
}

/// Recursively check field existence within a JSON object.
/// When the exists value is an object, parse the actual string as JSON
/// and check each field's existence recursively (Mountebank compatible).
fn check_exists_json_recursive(expected: &serde_json::Value, actual_str: &str) -> bool {
    match expected {
        serde_json::Value::Bool(should_exist) => {
            let exists = !actual_str.is_empty();
            exists == *should_exist
        }
        serde_json::Value::Object(expected_obj) => {
            let actual_json: serde_json::Value = match serde_json::from_str(actual_str) {
                Ok(v) => v,
                Err(_) => {
                    // If we can't parse as JSON, check if any field expects non-existence
                    return expected_obj
                        .values()
                        .all(|v| v == &serde_json::Value::Bool(false));
                }
            };

            for (key, expected_val) in expected_obj {
                match expected_val {
                    serde_json::Value::Bool(should_exist) => {
                        let exists = actual_json.get(key).is_some();
                        if exists != *should_exist {
                            return false;
                        }
                    }
                    serde_json::Value::Object(_) => {
                        // Recurse into nested object
                        let nested_str = match actual_json.get(key) {
                            Some(v) => json_value_to_string(v),
                            None => return false,
                        };
                        if !check_exists_json_recursive(expected_val, &nested_str) {
                            return false;
                        }
                    }
                    _ => {
                        // Non-boolean, non-object values are treated as true (field should exist)
                        if actual_json.get(key).is_none() {
                            return false;
                        }
                    }
                }
            }
            true
        }
        _ => {
            // Non-boolean, non-object exists values: treat as "should exist" = true
            !actual_str.is_empty()
        }
    }
}

/// Check exists predicate - verifies field presence or absence
/// Supports: method, path, body, query, headers, form, requestFrom, ip
/// When a field's value is an object (not a boolean), parse the actual value as JSON
/// and recursively check field existence within it (Mountebank compatible).
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_exists_predicate(
    obj: &HashMap<String, serde_json::Value>,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &str,
    request_from: Option<&str>,
    client_ip: Option<&str>,
    form: Option<&HashMap<String, String>>,
    key_case_sensitive: bool,
) -> bool {
    // Helper for key comparison based on keyCaseSensitive
    let key_matches = |expected_key: &str, actual_key: &str| -> bool {
        if key_case_sensitive {
            expected_key == actual_key
        } else {
            expected_key.eq_ignore_ascii_case(actual_key)
        }
    };

    // Check method exists (always present in HTTP requests)
    if let Some(expected) = obj.get("method") {
        let should_exist = expected.as_bool().unwrap_or(true);
        let exists = !method.is_empty();
        if exists != should_exist {
            return false;
        }
    }

    // Check path exists (always present in HTTP requests)
    if let Some(expected) = obj.get("path") {
        let should_exist = expected.as_bool().unwrap_or(true);
        let exists = !path.is_empty();
        if exists != should_exist {
            return false;
        }
    }

    // Check requestFrom exists
    if let Some(expected) = obj.get("requestFrom") {
        let should_exist = expected.as_bool().unwrap_or(true);
        let exists = request_from.is_some_and(|v| !v.is_empty());
        if exists != should_exist {
            return false;
        }
    }

    // Check ip exists
    if let Some(expected) = obj.get("ip") {
        let should_exist = expected.as_bool().unwrap_or(true);
        let exists = client_ip.is_some_and(|v| !v.is_empty());
        if exists != should_exist {
            return false;
        }
    }

    // Check body exists - supports both boolean and object values
    if let Some(expected) = obj.get("body") {
        if !check_exists_json_recursive(expected, body) {
            return false;
        }
    }

    // Check query parameters exist
    if let Some(expected_query) = obj.get("query").and_then(|v| v.as_object()) {
        for (key, should_exist_val) in expected_query {
            let should_exist = should_exist_val.as_bool().unwrap_or(true);
            let exists = query.iter().any(|(k, _)| key_matches(key, k));
            if exists != should_exist {
                return false;
            }
        }
    }

    // Check headers exist
    if let Some(expected_headers) = obj.get("headers").and_then(|v| v.as_object()) {
        for (key, should_exist_val) in expected_headers {
            let should_exist = should_exist_val.as_bool().unwrap_or(true);
            let exists = headers.iter().any(|(k, _)| key_matches(key, k));
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
            let exists = actual_form.iter().any(|(k, _)| key_matches(key, k));
            if exists != should_exist {
                return false;
            }
        }
    }

    true
}

/// Recursively apply a comparison function when the expected value is a JSON object.
/// Parses the actual string as JSON and compares each field recursively.
/// For leaf values, converts both to strings and applies the comparison function.
/// When `deep_equals` is true, also verifies no extra keys exist in actual objects
/// and arrays are compared structurally (same length, element-wise).
/// `key_case_sensitive` controls whether JSON object key lookups are case-sensitive.
/// `apply_except` is applied to leaf values (not raw JSON strings) to avoid breaking
/// JSON structure before parsing.
pub(crate) fn compare_json_recursive<F>(
    expected: &serde_json::Value,
    actual_str: &str,
    compare: &F,
    deep_equals: bool,
    key_case_sensitive: bool,
    apply_except: &dyn Fn(&str) -> String,
) -> bool
where
    F: Fn(&str, &str) -> bool,
{
    match expected {
        serde_json::Value::Object(expected_obj) => {
            let actual_json: serde_json::Value = match serde_json::from_str(actual_str) {
                Ok(v) => v,
                Err(_) => return false,
            };

            let actual_obj = match actual_json.as_object() {
                Some(obj) => obj,
                None => return false,
            };

            // For deepEquals, actual must have exactly the same keys
            if deep_equals && expected_obj.len() != actual_obj.len() {
                return false;
            }

            for (key, expected_val) in expected_obj {
                let actual_val = if key_case_sensitive {
                    actual_obj.get(key)
                } else {
                    actual_obj
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(key))
                        .map(|(_, v)| v)
                };

                let actual_val = match actual_val {
                    Some(v) => v,
                    None => return false,
                };

                let actual_val_str = json_value_to_string(actual_val);
                if !compare_json_recursive(
                    expected_val,
                    &actual_val_str,
                    compare,
                    deep_equals,
                    key_case_sensitive,
                    apply_except,
                ) {
                    return false;
                }
            }
            true
        }
        serde_json::Value::Array(expected_arr) => {
            let actual_json: serde_json::Value = match serde_json::from_str(actual_str) {
                Ok(v) => v,
                Err(_) => return false,
            };

            let actual_arr = match actual_json.as_array() {
                Some(arr) => arr,
                None => return false,
            };

            if expected_arr.len() != actual_arr.len() {
                return false;
            }

            for (expected_elem, actual_elem) in expected_arr.iter().zip(actual_arr.iter()) {
                let actual_elem_str = json_value_to_string(actual_elem);
                if !compare_json_recursive(
                    expected_elem,
                    &actual_elem_str,
                    compare,
                    deep_equals,
                    key_case_sensitive,
                    apply_except,
                ) {
                    return false;
                }
            }
            true
        }
        _ => {
            let expected_str = json_value_to_string(expected);
            let actual_str = apply_except(actual_str);
            compare(&expected_str, &actual_str)
        }
    }
}

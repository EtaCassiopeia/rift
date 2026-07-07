//! Predicate types for matching requests against stubs.
//!
//! These are pure data types (no matching logic) so they can be shared across the
//! workspace — the proxy for matching, the linter for concrete-type validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single predicate: matcher parameters plus the operation to apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Predicate {
    #[serde(flatten)]
    pub parameters: PredicateParameters,
    #[serde(flatten)]
    pub operation: PredicateOperation,
}

/// The matching operation a predicate performs (Mountebank-compatible).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PredicateOperation {
    Equals(HashMap<String, serde_json::Value>),
    DeepEquals(HashMap<String, serde_json::Value>),
    Contains(HashMap<String, serde_json::Value>),
    StartsWith(HashMap<String, serde_json::Value>),
    EndsWith(HashMap<String, serde_json::Value>),
    Matches(HashMap<String, serde_json::Value>),
    Exists(HashMap<String, serde_json::Value>),
    Not(Box<Predicate>),
    Or(Vec<Predicate>),
    And(Vec<Predicate>),
    Inject(String),
}

/// Matcher parameters shared across operations (case sensitivity, selectors, etc.).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateParameters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_case_sensitive: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub except: String,
    #[serde(flatten)]
    pub selector: Option<PredicateSelector>,
}

/// A structured selector applied to the request body before matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PredicateSelector {
    XPath {
        selector: String,
        #[serde(rename = "ns", default, skip_serializing_if = "Option::is_none")]
        namespaces: Option<HashMap<String, String>>,
    },
    JsonPath {
        selector: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserializes_into_concrete_operation() {
        // The headline benefit for the linter: match on a typed operation instead of
        // poking at a serde_json::Value.
        let pred: Predicate =
            serde_json::from_value(json!({ "equals": { "path": "/hello" } })).unwrap();
        match pred.operation {
            PredicateOperation::Equals(fields) => {
                assert_eq!(fields.get("path").unwrap(), "/hello");
            }
            other => panic!("expected Equals, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_parameters_and_selector() {
        let value = json!({
            "equals": { "body": "x" },
            "caseSensitive": false,
            "jsonpath": { "selector": "$.id" }
        });
        let pred: Predicate = serde_json::from_value(value).unwrap();
        assert_eq!(pred.parameters.case_sensitive, Some(false));
        assert!(matches!(
            pred.parameters.selector,
            Some(PredicateSelector::JsonPath { .. })
        ));
        assert!(matches!(pred.operation, PredicateOperation::Equals(_)));

        // re-serializing keeps the camelCase wire shape
        let back = serde_json::to_value(&pred).unwrap();
        assert_eq!(back["caseSensitive"], json!(false));
        assert!(back["jsonpath"]["selector"] == json!("$.id"));
    }

    #[test]
    fn xpath_selector_round_trips_with_namespaces() {
        // The XPath selector carries the most fragile serde attributes: the enum's
        // `rename_all = "lowercase"` ("xpath") and `rename = "ns"` on namespaces.
        let value = json!({
            "equals": { "body": "x" },
            "xpath": { "selector": "//a:user", "ns": { "a": "urn:y" } }
        });
        let pred: Predicate = serde_json::from_value(value).unwrap();
        let Some(PredicateSelector::XPath {
            selector,
            namespaces,
        }) = &pred.parameters.selector
        else {
            panic!("expected an XPath selector");
        };
        assert_eq!(selector, "//a:user");
        assert_eq!(namespaces.as_ref().unwrap().get("a").unwrap(), "urn:y");

        let back = serde_json::to_value(&pred).unwrap();
        assert_eq!(back["xpath"]["selector"], json!("//a:user"));
        assert_eq!(
            back["xpath"]["ns"]["a"],
            json!("urn:y"),
            "the `ns` rename is preserved"
        );
    }

    #[test]
    fn except_and_key_case_sensitive_round_trip() {
        let pred: Predicate = serde_json::from_value(json!({
            "matches": { "path": "/x" },
            "except": "^/skip",
            "keyCaseSensitive": true
        }))
        .unwrap();
        assert_eq!(pred.parameters.except, "^/skip");
        assert_eq!(pred.parameters.key_case_sensitive, Some(true));
        let back = serde_json::to_value(&pred).unwrap();
        assert_eq!(back["except"], json!("^/skip"));
        assert_eq!(back["keyCaseSensitive"], json!(true));

        // empty `except` is omitted from the wire form (skip_serializing_if)
        let empty: Predicate =
            serde_json::from_value(json!({ "matches": { "path": "/x" } })).unwrap();
        assert!(
            serde_json::to_value(&empty)
                .unwrap()
                .get("except")
                .is_none()
        );
    }

    #[test]
    fn nests_logical_operators() {
        let pred: Predicate = serde_json::from_value(json!({
            "and": [
                { "equals": { "method": "GET" } },
                { "not": { "exists": { "headers": { "X": true } } } }
            ]
        }))
        .unwrap();
        let PredicateOperation::And(subs) = pred.operation else {
            panic!("expected And");
        };
        assert_eq!(subs.len(), 2);
        assert!(matches!(subs[1].operation, PredicateOperation::Not(_)));
    }
}

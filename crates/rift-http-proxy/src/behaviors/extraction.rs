//! Extraction methods: regex, JSONPath, XPath.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Method for extracting values from source
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum ExtractionMethod {
    /// Regular expression with capture groups
    Regex { selector: String },
    /// JSONPath expression
    #[serde(rename = "jsonpath")]
    JsonPath { selector: String },
    /// XPath expression for XML
    #[serde(rename = "xpath")]
    XPath { selector: String },
}

impl ExtractionMethod {
    /// Apply extraction to a value
    pub fn extract(&self, value: &str) -> Option<String> {
        match self {
            ExtractionMethod::Regex { selector } => {
                let re = Regex::new(selector).ok()?;
                if let Some(caps) = re.captures(value) {
                    // Return first capture group if exists, otherwise full match
                    caps.get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str().to_string())
                } else {
                    None
                }
            }
            ExtractionMethod::JsonPath { selector } => extract_jsonpath(value, selector),
            ExtractionMethod::XPath { selector } => extract_xpath(value, selector),
        }
    }
}

/// Extract value using JSONPath
/// Used by copy behaviors and predicate jsonpath parameter
pub fn extract_jsonpath(json_str: &str, path: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Simple JSONPath implementation (supports basic paths like $.field, $.array[0])
    let path = path.trim_start_matches('$').trim_start_matches('.');

    let mut current = &json;
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }

        // Check for array index
        if let Some(bracket_pos) = part.find('[') {
            let field = &part[..bracket_pos];
            let index_str = &part[bracket_pos + 1..part.len() - 1];

            if !field.is_empty() {
                current = current.get(field)?;
            }

            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(part)?;
        }
    }

    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null => Some("null".to_string()),
        _ => Some(current.to_string()),
    }
}

/// Extract value using XPath
/// Used by copy behaviors and predicate xpath parameter
pub fn extract_xpath(xml_str: &str, path: &str) -> Option<String> {
    use sxd_document::parser;
    use sxd_xpath::{evaluate_xpath, Value};

    let package = parser::parse(xml_str).ok()?;
    let document = package.as_document();

    match evaluate_xpath(&document, path) {
        Ok(Value::String(s)) => Some(s),
        Ok(Value::Number(n)) => Some(n.to_string()),
        Ok(Value::Boolean(b)) => Some(b.to_string()),
        Ok(Value::Nodeset(nodes)) => nodes.iter().next().map(|n| n.string_value()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_regex() {
        let method = ExtractionMethod::Regex {
            selector: r"/users/(\d+)".to_string(),
        };
        assert_eq!(method.extract("/users/123"), Some("123".to_string()));
        assert_eq!(method.extract("/posts/456"), None);
    }

    #[test]
    fn test_extraction_regex_full_match() {
        let method = ExtractionMethod::Regex {
            selector: r".*".to_string(),
        };
        assert_eq!(
            method.extract("hello world"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn test_extraction_jsonpath() {
        let method = ExtractionMethod::JsonPath {
            selector: "$.user.name".to_string(),
        };
        let json = r#"{"user": {"name": "Alice", "age": 30}}"#;
        assert_eq!(method.extract(json), Some("Alice".to_string()));
    }

    #[test]
    fn test_extraction_jsonpath_array() {
        let method = ExtractionMethod::JsonPath {
            selector: "$.items[0]".to_string(),
        };
        let json = r#"{"items": ["first", "second"]}"#;
        assert_eq!(method.extract(json), Some("first".to_string()));
    }
}

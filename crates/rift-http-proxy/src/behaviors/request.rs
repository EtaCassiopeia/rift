//! Request context for behavior processing.

use std::collections::HashMap;

/// Convert a header name to title case (e.g., "content-type" -> "Content-Type").
///
/// This is used for Mountebank compatibility, which expects title-cased header names.
pub fn header_to_title_case(name: &str) -> String {
    let mut title_case = String::with_capacity(name.len());
    for part in name.split_inclusive('-') {
        let mut chars = part.chars();
        if let Some(first_char) = chars.next() {
            title_case.push(first_char.to_ascii_uppercase());
        }
        title_case.push_str(chars.as_str());
    }
    title_case
}

/// Request context for behavior processing
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

impl RequestContext {
    /// Create from hyper request parts
    pub fn from_request(
        method: &str,
        uri: &hyper::Uri,
        headers: &hyper::HeaderMap,
        body: Option<&str>,
    ) -> Self {
        let mut query_map = HashMap::new();
        if let Some(query) = uri.query() {
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    query_map.insert(
                        key.to_string(),
                        urlencoding::decode(value).unwrap_or_default().to_string(),
                    );
                }
            }
        }

        let mut header_map = HashMap::new();
        for (name, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                header_map.insert(header_to_title_case(name.as_str()), v.to_string());
            }
        }

        Self {
            method: method.to_string(),
            path: uri.path().to_string(),
            query: query_map,
            headers: header_map,
            body: body.map(|s| s.to_string()),
        }
    }
}

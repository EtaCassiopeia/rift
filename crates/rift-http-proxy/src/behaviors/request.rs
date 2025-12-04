//! Request context for behavior processing.

use std::collections::HashMap;

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
                // Preserve header case for Mountebank compatibility
                // Convert from hyper's lowercase to title case (e.g., "content-type" -> "Content-Type")
                let title_case_name = name
                    .as_str()
                    .split('-')
                    .map(|part| {
                        let mut chars: Vec<char> = part.chars().collect();
                        if let Some(first) = chars.first_mut() {
                            *first = first.to_uppercase().next().unwrap_or(*first);
                        }
                        chars.into_iter().collect::<String>()
                    })
                    .collect::<Vec<String>>()
                    .join("-");
                header_map.insert(title_case_name, v.to_string());
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

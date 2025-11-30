//! Mountebank-compatible imposter management for Rift.
//!
//! This module provides:
//! - `ImposterManager`: Lifecycle management for imposters
//! - `Imposter`: Individual imposter with its own port, rules, and state
//! - `ImposterConfig`: Configuration for creating imposters
//!
//! Each imposter binds to its own TCP port and maintains isolated state.

use crate::behaviors::{
    apply_copy_behaviors, apply_decorate, HasRepeatBehavior, RequestContext, ResponseBehaviors,
    ResponseCycler,
};
use crate::recording::{ProxyMode, RecordedResponse, RecordingStore, RequestSignature};
#[cfg(feature = "javascript")]
use crate::scripting::{execute_mountebank_inject, MountebankRequest};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Global HTTP client for proxy requests
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(0) // Disable connection pooling to avoid stale connections
            .build()
            .expect("Failed to create HTTP client")
    })
}

/// Recorded request for imposter
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRequest {
    pub request_from: String,
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub timestamp: String,
}

/// Stub definition (Mountebank-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stub {
    #[serde(default)]
    pub predicates: Vec<serde_json::Value>,
    pub responses: Vec<StubResponse>,
    /// Optional scenario name for documentation/organization (Mountebank compatible)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_name: Option<String>,
}

/// Response within a stub - wrapper type that handles various formats
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "StubResponseRaw", into = "StubResponseRaw")]
pub enum StubResponse {
    Is {
        is: IsResponse,
        #[serde(rename = "_behaviors", skip_serializing_if = "Option::is_none")]
        behaviors: Option<serde_json::Value>,
    },
    Proxy {
        proxy: ProxyResponse,
    },
    Inject {
        inject: String,
    },
    Fault {
        fault: String,
    },
}

/// Raw deserialization type that handles multiple JSON formats for stub responses
/// Supports:
/// - Standard Mountebank format with `is`, `proxy`, `inject`, or `fault` fields
/// - Formats with `behaviors` (without underscore) or `_behaviors`
/// - Formats with `proxy: null` alongside `is` (ignored)
/// - `statusCode` as either string or number
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StubResponseRaw {
    #[serde(skip_serializing_if = "Option::is_none")]
    is: Option<IsResponseRaw>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxy: Option<ProxyResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fault: Option<String>,
    /// Mountebank-style behaviors (with underscore prefix)
    #[serde(rename = "_behaviors", skip_serializing_if = "Option::is_none")]
    underscore_behaviors: Option<serde_json::Value>,
    /// Alternative behaviors field (without underscore, used by some tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    behaviors: Option<serde_json::Value>,
}

/// Raw IsResponse that handles statusCode as string or number
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct IsResponseRaw {
    #[serde(default = "default_status_code_raw", deserialize_with = "deserialize_status_code")]
    status_code: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<serde_json::Value>,
}

fn default_status_code_raw() -> u16 {
    200
}

/// Deserialize statusCode from either a number or a string
fn deserialize_status_code<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| D::Error::custom("invalid status code number")),
        serde_json::Value::String(s) => s
            .parse::<u16>()
            .map_err(|_| D::Error::custom(format!("invalid status code string: {s}"))),
        _ => Err(D::Error::custom("statusCode must be a number or string")),
    }
}

impl From<StubResponseRaw> for StubResponse {
    fn from(raw: StubResponseRaw) -> Self {
        // Priority: is > proxy > inject > fault
        if let Some(is_raw) = raw.is {
            // Merge behaviors: prefer _behaviors, fall back to behaviors
            let behaviors = raw.underscore_behaviors.or_else(|| {
                // Convert array format to object format if needed
                raw.behaviors.and_then(normalize_behaviors)
            });
            StubResponse::Is {
                is: IsResponse {
                    status_code: is_raw.status_code,
                    headers: is_raw.headers,
                    body: is_raw.body,
                },
                behaviors,
            }
        } else if let Some(proxy) = raw.proxy {
            StubResponse::Proxy { proxy }
        } else if let Some(inject) = raw.inject {
            StubResponse::Inject { inject }
        } else if let Some(fault) = raw.fault {
            StubResponse::Fault { fault }
        } else {
            // Default to empty Is response
            StubResponse::Is {
                is: IsResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: None,
                },
                behaviors: None,
            }
        }
    }
}

impl From<StubResponse> for StubResponseRaw {
    fn from(response: StubResponse) -> Self {
        match response {
            StubResponse::Is { is, behaviors } => StubResponseRaw {
                is: Some(IsResponseRaw {
                    status_code: is.status_code,
                    headers: is.headers,
                    body: is.body,
                }),
                proxy: None,
                inject: None,
                fault: None,
                underscore_behaviors: behaviors,
                behaviors: None,
            },
            StubResponse::Proxy { proxy } => StubResponseRaw {
                is: None,
                proxy: Some(proxy),
                inject: None,
                fault: None,
                underscore_behaviors: None,
                behaviors: None,
            },
            StubResponse::Inject { inject } => StubResponseRaw {
                is: None,
                proxy: None,
                inject: Some(inject),
                fault: None,
                underscore_behaviors: None,
                behaviors: None,
            },
            StubResponse::Fault { fault } => StubResponseRaw {
                is: None,
                proxy: None,
                inject: None,
                fault: Some(fault),
                underscore_behaviors: None,
                behaviors: None,
            },
        }
    }
}

/// Normalize behaviors from array format to object format
/// Some tools use `behaviors: [{"wait": ...}, {"decorate": ...}]` instead of
/// `_behaviors: {"wait": ..., "decorate": ...}`
fn normalize_behaviors(value: serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Array(arr) => {
            // Convert array of behavior objects to a single merged object
            let mut merged = serde_json::Map::new();
            for item in arr {
                if let serde_json::Value::Object(obj) = item {
                    for (k, v) in obj {
                        merged.insert(k, v);
                    }
                }
            }
            if merged.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(merged))
            }
        }
        serde_json::Value::Object(_) => Some(value),
        _ => None,
    }
}

impl HasRepeatBehavior for StubResponse {
    fn get_repeat(&self) -> Option<u32> {
        match self {
            StubResponse::Is { behaviors, .. } => behaviors
                .as_ref()
                .and_then(|b| b.get("repeat"))
                .and_then(|r| r.as_u64())
                .map(|r| r as u32),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsResponse {
    #[serde(default = "default_status_code")]
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

fn default_status_code() -> u16 {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyResponse {
    pub to: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub predicate_generators: Vec<serde_json::Value>,
    #[serde(default)]
    pub add_wait_behavior: bool,
    #[serde(default)]
    pub inject_headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_decorate_behavior: Option<String>,
}

/// Configuration for creating an imposter
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImposterConfig {
    /// Port for the imposter. If not specified, an available port will be auto-assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub record_requests: bool,
    #[serde(default)]
    pub stubs: Vec<Stub>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_response: Option<IsResponse>,
    /// Allow CORS headers (Mountebank compatible)
    #[serde(default, skip_serializing_if = "std::ops::Not::not", alias = "allowCORS")]
    pub allow_cors: bool,
    /// Service name for documentation (optional metadata)
    #[serde(skip_serializing_if = "Option::is_none", alias = "service_name")]
    pub service_name: Option<String>,
    /// Service info for documentation (optional metadata, stored as-is)
    #[serde(skip_serializing_if = "Option::is_none", alias = "service_info")]
    pub service_info: Option<serde_json::Value>,
}

fn default_protocol() -> String {
    "http".to_string()
}

/// Runtime state of an imposter
pub struct Imposter {
    pub config: ImposterConfig,
    /// Mutable stubs (can be modified at runtime)
    pub stubs: RwLock<Vec<Stub>>,
    /// Response cycling state (for future use with response arrays)
    #[allow(dead_code)]
    pub response_cycler: ResponseCycler,
    /// Recording store for proxy responses (for future proxy mode support)
    #[allow(dead_code)]
    pub recording_store: Arc<RecordingStore>,
    /// Recorded requests (if record_requests is true)
    pub recorded_requests: RwLock<Vec<RecordedRequest>>,
    /// Request count
    pub request_count: AtomicU64,
    /// Whether imposter is enabled
    pub enabled: AtomicBool,
    /// Creation timestamp (for future metrics/admin display)
    #[allow(dead_code)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Shutdown signal sender (for future graceful shutdown)
    #[allow(dead_code)]
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl Imposter {
    /// Create a new imposter from config
    pub fn new(config: ImposterConfig) -> Self {
        let stubs = config.stubs.clone();

        // Extract proxy mode from stubs (use first proxy response's mode)
        let proxy_mode = Self::extract_proxy_mode(&stubs);

        Self {
            config,
            stubs: RwLock::new(stubs),
            response_cycler: ResponseCycler::new(),
            recording_store: Arc::new(RecordingStore::new(proxy_mode)),
            recorded_requests: RwLock::new(Vec::new()),
            request_count: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
            created_at: chrono::Utc::now(),
            shutdown_tx: None,
        }
    }

    /// Extract proxy mode from stubs
    fn extract_proxy_mode(stubs: &[Stub]) -> ProxyMode {
        for stub in stubs {
            for response in &stub.responses {
                if let StubResponse::Proxy { proxy } = response {
                    return match proxy.mode.to_lowercase().as_str() {
                        "proxyonce" => ProxyMode::ProxyOnce,
                        "proxyalways" => ProxyMode::ProxyAlways,
                        "proxytransparent" | "" => ProxyMode::ProxyTransparent,
                        _ => ProxyMode::ProxyTransparent,
                    };
                }
            }
        }
        ProxyMode::ProxyTransparent
    }

    /// Find a matching stub for a request and return a cloned copy with its index
    pub fn find_matching_stub(
        &self,
        method: &str,
        path: &str,
        headers: &hyper::HeaderMap,
        query: Option<&str>,
        body: Option<&str>,
    ) -> Option<(Stub, usize)> {
        let stubs = self.stubs.read();
        let headers_map = Self::header_map_to_hashmap(headers);
        for (index, stub) in stubs.iter().enumerate() {
            if Self::stub_matches(stub, method, path, query, &headers_map, body) {
                return Some((stub.clone(), index));
            }
        }
        None
    }

    /// Convert hyper HeaderMap to HashMap<String, String>
    fn header_map_to_hashmap(headers: &hyper::HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }

    /// Check if a stub matches a request
    fn stub_matches(
        stub: &Stub,
        method: &str,
        path: &str,
        query: Option<&str>,
        headers: &HashMap<String, String>,
        body: Option<&str>,
    ) -> bool {
        // If no predicates, match everything
        if stub.predicates.is_empty() {
            return true;
        }

        // All predicates must match (implicit AND)
        for predicate in &stub.predicates {
            if !Self::predicate_matches(predicate, method, path, query, headers, body) {
                return false;
            }
        }
        true
    }

    /// Parse query string into a HashMap
    fn parse_query(query: Option<&str>) -> HashMap<String, String> {
        query
            .unwrap_or("")
            .split('&')
            .filter(|s| !s.is_empty())
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                Some((parts.next()?.to_string(), parts.next()?.to_string()))
            })
            .collect()
    }

    /// Check if a single predicate matches (Mountebank-compatible)
    /// Supports: equals, deepEquals, contains, startsWith, endsWith, matches, exists, not, or, and
    fn predicate_matches(
        predicate: &serde_json::Value,
        method: &str,
        path: &str,
        query: Option<&str>,
        headers: &HashMap<String, String>,
        body: Option<&str>,
    ) -> bool {
        let obj = match predicate.as_object() {
            Some(o) => o,
            None => return true,
        };

        // Get predicate options
        let case_sensitive = obj
            .get("caseSensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let except_pattern = obj.get("except").and_then(|v| v.as_str());

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
        let query_map = Self::parse_query(query);
        let body_str = body.unwrap_or("");

        // Handle logical "not" operator
        if let Some(not_pred) = obj.get("not") {
            return !Self::predicate_matches(not_pred, method, path, query, headers, body);
        }

        // Handle logical "or" operator
        if let Some(or_preds) = obj.get("or").and_then(|v| v.as_array()) {
            return or_preds
                .iter()
                .any(|p| Self::predicate_matches(p, method, path, query, headers, body));
        }

        // Handle logical "and" operator
        if let Some(and_preds) = obj.get("and").and_then(|v| v.as_array()) {
            return and_preds
                .iter()
                .all(|p| Self::predicate_matches(p, method, path, query, headers, body));
        }

        // Handle "equals" predicate (subset matching for objects)
        if let Some(equals) = obj.get("equals") {
            if !Self::check_predicate_fields(
                equals,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                |expected, actual| str_equals(expected, actual),
                false, // not deep equals
            ) {
                return false;
            }
        }

        // Handle "deepEquals" predicate (exact matching)
        if let Some(deep_equals) = obj.get("deepEquals") {
            if !Self::check_predicate_fields(
                deep_equals,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                |expected, actual| str_equals(expected, actual),
                true, // deep equals
            ) {
                return false;
            }
        }

        // Handle "contains" predicate
        if let Some(contains) = obj.get("contains") {
            if !Self::check_predicate_fields(
                contains,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                |expected, actual| str_contains(actual, expected),
                false,
            ) {
                return false;
            }
        }

        // Handle "startsWith" predicate
        if let Some(starts_with) = obj.get("startsWith") {
            if !Self::check_predicate_fields(
                starts_with,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                |expected, actual| str_starts_with(actual, expected),
                false,
            ) {
                return false;
            }
        }

        // Handle "endsWith" predicate
        if let Some(ends_with) = obj.get("endsWith") {
            if !Self::check_predicate_fields(
                ends_with,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                |expected, actual| str_ends_with(actual, expected),
                false,
            ) {
                return false;
            }
        }

        // Handle "matches" predicate (regex)
        if let Some(matches) = obj.get("matches") {
            if !Self::check_predicate_fields_regex(
                matches,
                method,
                path,
                &query_map,
                headers,
                body_str,
                &apply_except,
                case_sensitive,
            ) {
                return false;
            }
        }

        // Handle "exists" predicate
        if let Some(exists) = obj.get("exists") {
            if !Self::check_exists_predicate(exists, &query_map, headers, body_str) {
                return false;
            }
        }

        true
    }

    /// Check predicate fields against request values
    fn check_predicate_fields<F>(
        predicate_value: &serde_json::Value,
        method: &str,
        path: &str,
        query: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        body: &str,
        apply_except: &impl Fn(&str) -> String,
        compare: F,
        deep_equals: bool,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        let obj = match predicate_value.as_object() {
            Some(o) => o,
            None => return true,
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
                    match query.get(key) {
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
                    // Headers are case-insensitive for key lookup
                    let actual = headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(key))
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
    fn check_predicate_fields_regex(
        predicate_value: &serde_json::Value,
        method: &str,
        path: &str,
        query: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        body: &str,
        apply_except: &impl Fn(&str) -> String,
        case_sensitive: bool,
    ) -> bool {
        let obj = match predicate_value.as_object() {
            Some(o) => o,
            None => return true,
        };

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

        // Check query parameters
        if let Some(expected_query) = obj.get("query").and_then(|v| v.as_object()) {
            for (key, pattern_val) in expected_query {
                let pattern = match pattern_val {
                    serde_json::Value::String(s) => s.as_str(),
                    _ => continue,
                };
                if let Some(re) = build_regex(pattern) {
                    match query.get(key) {
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
                        .find(|(k, _)| k.eq_ignore_ascii_case(key))
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
    fn check_exists_predicate(
        predicate_value: &serde_json::Value,
        query: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        body: &str,
    ) -> bool {
        let obj = match predicate_value.as_object() {
            Some(o) => o,
            None => return true,
        };

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
                let exists = headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case(key));
                if exists != should_exist {
                    return false;
                }
            }
        }

        true
    }

    /// Execute a stub and get the response with behaviors
    /// Returns (status, headers, body, behaviors, is_fault)
    #[allow(clippy::type_complexity)]
    pub fn execute_stub(
        &self,
        stub: &Stub,
        stub_index: usize,
    ) -> Option<(
        u16,
        HashMap<String, String>,
        String,
        Option<serde_json::Value>,
        bool,
    )> {
        if stub.responses.is_empty() {
            return None;
        }

        // Use response cycler with per-response repeat values
        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .get_response_index_with_per_response_repeat(&rule_id, &stub.responses);

        let response = stub.responses.get(response_index)?;

        match response {
            StubResponse::Is { is, behaviors } => {
                let mut headers = is.headers.clone();

                let body = is
                    .body
                    .as_ref()
                    .map(|b| {
                        if b.is_string() {
                            b.as_str().unwrap_or("").to_string()
                        } else {
                            // Set content-type for JSON
                            if !headers.contains_key("content-type")
                                && !headers.contains_key("Content-Type")
                            {
                                headers.insert(
                                    "Content-Type".to_string(),
                                    "application/json".to_string(),
                                );
                            }
                            serde_json::to_string(b).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();

                Some((is.status_code, headers, body, behaviors.clone(), false))
            }
            StubResponse::Fault { fault } => {
                // Return special marker for fault - will be handled by caller
                Some((
                    0,
                    HashMap::new(),
                    fault.clone(),
                    None,
                    true, // is_fault = true
                ))
            }
            StubResponse::Proxy { .. } => None, // Handled separately in handle_imposter_request
            StubResponse::Inject { .. } => None, // Inject not implemented yet
        }
    }

    /// Check if a stub response is a proxy and return the proxy config
    /// Note: This peeks at the current response without advancing the cycler
    pub fn get_proxy_response(&self, stub: &Stub, stub_index: usize) -> Option<ProxyResponse> {
        if stub.responses.is_empty() {
            return None;
        }

        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .peek_response_index(&rule_id, stub.responses.len());

        let response = stub.responses.get(response_index)?;

        match response {
            StubResponse::Proxy { proxy } => Some(proxy.clone()),
            _ => None,
        }
    }

    /// Advance the response cycler for a proxy response
    /// This should be called after successfully handling a proxy response
    pub fn advance_cycler_for_proxy(&self, stub: &Stub, stub_index: usize) {
        let rule_id = format!("stub_{stub_index}");
        self.response_cycler
            .advance_for_proxy(&rule_id, stub.responses.len());
    }

    /// Check if a stub response is an inject and return the inject function
    /// Note: This peeks at the current response without advancing the cycler
    #[allow(dead_code)] // Used with javascript feature
    pub fn get_inject_response(&self, stub: &Stub, stub_index: usize) -> Option<String> {
        if stub.responses.is_empty() {
            return None;
        }

        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .peek_response_index(&rule_id, stub.responses.len());

        let response = stub.responses.get(response_index)?;

        match response {
            StubResponse::Inject { inject } => Some(inject.clone()),
            _ => None,
        }
    }

    /// Advance the response cycler for an inject response
    /// This should be called after successfully handling an inject response
    #[allow(dead_code)] // Used with javascript feature
    pub fn advance_cycler_for_inject(&self, stub: &Stub, stub_index: usize) {
        let rule_id = format!("stub_{stub_index}");
        self.response_cycler
            .advance_for_proxy(&rule_id, stub.responses.len());
    }

    /// Generate predicates from request based on predicateGenerators config
    fn generate_predicates_from_request(
        &self,
        generators: &[serde_json::Value],
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: Option<&str>,
    ) -> Vec<serde_json::Value> {
        let mut predicates = Vec::new();

        for gen in generators {
            let gen_obj = match gen.as_object() {
                Some(obj) => obj,
                None => continue,
            };

            // Get the matches config
            let matches = match gen_obj.get("matches").and_then(|m| m.as_object()) {
                Some(m) => m,
                None => continue,
            };

            // Get options
            let case_sensitive = gen_obj
                .get("caseSensitive")
                .and_then(|c| c.as_bool())
                .unwrap_or(true);
            let predicate_operator = gen_obj
                .get("predicateOperator")
                .and_then(|p| p.as_str())
                .unwrap_or("equals");
            let except_pattern = gen_obj.get("except").and_then(|e| e.as_str());

            // Build predicate values
            let mut pred_values = serde_json::Map::new();

            // Handle path
            if matches
                .get("path")
                .and_then(|p| p.as_bool())
                .unwrap_or(false)
            {
                let mut path_val = path.to_string();
                // Apply except pattern if present
                if let Some(pattern) = except_pattern {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        path_val = re.replace_all(&path_val, "").to_string();
                    }
                }
                pred_values.insert("path".to_string(), serde_json::Value::String(path_val));
            }

            // Handle method
            if matches
                .get("method")
                .and_then(|m| m.as_bool())
                .unwrap_or(false)
            {
                pred_values.insert(
                    "method".to_string(),
                    serde_json::Value::String(method.to_string()),
                );
            }

            // Handle headers
            if let Some(header_matches) = matches.get("headers").and_then(|h| h.as_object()) {
                let mut header_preds = serde_json::Map::new();
                for (header_name, should_match) in header_matches {
                    if should_match.as_bool().unwrap_or(false) {
                        if let Some(header_value) = headers.get(header_name) {
                            header_preds.insert(
                                header_name.clone(),
                                serde_json::Value::String(header_value.clone()),
                            );
                        }
                    }
                }
                if !header_preds.is_empty() {
                    pred_values.insert(
                        "headers".to_string(),
                        serde_json::Value::Object(header_preds),
                    );
                }
            }

            // Handle body
            if matches
                .get("body")
                .and_then(|b| b.as_bool())
                .unwrap_or(false)
            {
                if let Some(body_str) = body {
                    let mut body_val = body_str.to_string();
                    // Apply except pattern if present
                    if let Some(pattern) = except_pattern {
                        if let Ok(re) = regex::Regex::new(pattern) {
                            body_val = re.replace_all(&body_val, "").to_string();
                        }
                    }
                    pred_values.insert("body".to_string(), serde_json::Value::String(body_val));
                }
            }

            if pred_values.is_empty() {
                continue;
            }

            // Build the predicate with the operator
            let mut predicate = serde_json::Map::new();
            predicate.insert(
                predicate_operator.to_string(),
                serde_json::Value::Object(pred_values),
            );

            // Add caseSensitive if not default
            if !case_sensitive {
                predicate.insert("caseSensitive".to_string(), serde_json::Value::Bool(false));
            }

            predicates.push(serde_json::Value::Object(predicate));
        }

        predicates
    }

    /// Create a stub from a recorded proxy response
    fn create_stub_from_proxy_response(
        &self,
        predicates: Vec<serde_json::Value>,
        status: u16,
        headers: &HashMap<String, String>,
        body: &str,
        latency_ms: Option<u64>,
        decorate_fn: Option<String>,
    ) -> Stub {
        let mut response_headers = headers.clone();
        // Filter out hop-by-hop headers
        response_headers.retain(|k, _| {
            let k_lower = k.to_lowercase();
            k_lower != "transfer-encoding" && k_lower != "connection" && k_lower != "keep-alive"
        });

        let body_value = if body.is_empty() {
            None
        } else {
            // Try to parse as JSON, otherwise store as string
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(body) {
                Some(json_val)
            } else {
                Some(serde_json::Value::String(body.to_string()))
            }
        };

        let is_response = IsResponse {
            status_code: status,
            headers: response_headers,
            body: body_value,
        };

        // Build behaviors object if needed
        let behaviors = if latency_ms.is_some() || decorate_fn.is_some() {
            let mut behaviors_obj = serde_json::Map::new();
            if let Some(ms) = latency_ms {
                behaviors_obj.insert("wait".to_string(), serde_json::json!(ms));
            }
            if let Some(fn_str) = decorate_fn {
                behaviors_obj.insert("decorate".to_string(), serde_json::json!(fn_str));
            }
            Some(serde_json::Value::Object(behaviors_obj))
        } else {
            None
        };

        Stub {
            predicates,
            responses: vec![StubResponse::Is {
                is: is_response,
                behaviors,
            }],
            scenario_name: None,
        }
    }

    /// Insert a generated stub at the specified index
    pub fn insert_generated_stub(&self, stub: Stub, before_index: usize) {
        let mut stubs = self.stubs.write();
        let index = before_index.min(stubs.len());
        stubs.insert(index, stub);
        debug!("Inserted generated stub at index {}", index);
    }

    /// Forward a request through proxy and optionally record the response
    pub async fn handle_proxy_request(
        &self,
        proxy_config: &ProxyResponse,
        method: &str,
        uri: &hyper::Uri,
        headers: &HashMap<String, String>,
        body: Option<&str>,
        stub_index: usize,
    ) -> Result<(u16, HashMap<String, String>, String, Option<u64>), String> {
        let client = get_http_client();

        info!("Proxy config - addDecorateBehavior: {:?}, addWaitBehavior: {}, predicateGenerators: {:?}",
            proxy_config.add_decorate_behavior, proxy_config.add_wait_behavior, proxy_config.predicate_generators);

        // Build the proxy URL
        let target_url = format!(
            "{}{}{}",
            proxy_config.to,
            uri.path(),
            uri.query().map(|q| format!("?{q}")).unwrap_or_default()
        );

        debug!("Proxy request to: {}", target_url);

        // Create request signature for recording
        let signature = RequestSignature::new(method, uri.path(), uri.query(), &[]);

        // Check if we should replay cached response (based on proxy mode)
        if !self.recording_store.should_proxy(&signature) {
            if let Some(recorded) = self.recording_store.get_recorded(&signature) {
                debug!("Returning recorded proxy response (proxyOnce mode)");
                let headers: HashMap<String, String> = recorded.headers.clone();
                let body = String::from_utf8_lossy(&recorded.body).to_string();
                return Ok((recorded.status, headers, body, recorded.latency_ms));
            }
        }

        // Forward the request
        let start = Instant::now();

        let mut request = match method.to_uppercase().as_str() {
            "GET" => client.get(&target_url),
            "POST" => client.post(&target_url),
            "PUT" => client.put(&target_url),
            "DELETE" => client.delete(&target_url),
            "PATCH" => client.patch(&target_url),
            "HEAD" => client.head(&target_url),
            _ => client.get(&target_url),
        };

        // Copy headers (excluding host)
        for (key, value) in headers {
            let key_lower = key.to_lowercase();
            if key_lower != "host" && key_lower != "content-length" {
                request = request.header(key, value);
            }
        }

        // Add inject headers
        for (key, value) in &proxy_config.inject_headers {
            request = request.header(key, value);
        }

        // Add body if present
        if let Some(body_str) = body {
            request = request.body(body_str.to_string());
        }

        // Send request
        let response = request.send().await.map_err(|e| e.to_string())?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let status = response.status().as_u16();
        let response_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body_bytes = response.bytes().await.map_err(|e| e.to_string())?;
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        // Record the response
        let recorded_response = RecordedResponse {
            status,
            headers: response_headers.clone(),
            body: body_bytes.to_vec(),
            latency_ms: if proxy_config.add_wait_behavior {
                Some(latency_ms)
            } else {
                None
            },
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        self.recording_store.record(signature, recorded_response);

        // Generate and insert stub if predicateGenerators, addWaitBehavior, or addDecorateBehavior is configured
        // (Mountebank generates stubs automatically when these are enabled)
        if !proxy_config.predicate_generators.is_empty()
            || proxy_config.add_wait_behavior
            || proxy_config.add_decorate_behavior.is_some()
        {
            let predicates = if !proxy_config.predicate_generators.is_empty() {
                self.generate_predicates_from_request(
                    &proxy_config.predicate_generators,
                    method,
                    uri.path(),
                    headers,
                    body,
                )
            } else {
                // No predicateGenerators, generate empty predicates (matches all requests)
                vec![]
            };

            let latency_for_stub = if proxy_config.add_wait_behavior {
                Some(latency_ms)
            } else {
                None
            };

            // Note: addDecorateBehavior is added to the SAVED stub's behaviors,
            // not applied to the first (live proxy) response. This matches Mountebank's behavior.
            // The decoration will be applied when the saved stub is used for subsequent requests.

            let new_stub = self.create_stub_from_proxy_response(
                predicates,
                status,
                &response_headers,
                &body_str,
                latency_for_stub,
                proxy_config.add_decorate_behavior.clone(),
            );

            // Insert the new stub BEFORE the proxy stub
            self.insert_generated_stub(new_stub, stub_index);
            debug!("Generated stub from proxy response for path {}", uri.path());
        }

        Ok((
            status,
            response_headers,
            body_str,
            if proxy_config.add_wait_behavior {
                Some(latency_ms)
            } else {
                None
            },
        ))
    }

    /// Record a request
    pub fn record_request(&self, req: &RecordedRequest) {
        if self.config.record_requests {
            let mut requests = self.recorded_requests.write();
            requests.push(req.clone());
        }
    }

    /// Get recorded requests
    pub fn get_recorded_requests(&self) -> Vec<RecordedRequest> {
        self.recorded_requests.read().clone()
    }

    /// Clear recorded requests
    pub fn clear_recorded_requests(&self) {
        self.recorded_requests.write().clear();
        // Reset request count to match Mountebank behavior
        self.request_count.store(0, Ordering::SeqCst);
    }

    /// Clear saved proxy responses
    pub fn clear_proxy_responses(&self) {
        self.recording_store.clear();
    }

    /// Increment request count
    pub fn increment_request_count(&self) -> u64 {
        self.request_count.fetch_add(1, Ordering::SeqCst)
    }

    /// Get request count
    pub fn get_request_count(&self) -> u64 {
        self.request_count.load(Ordering::SeqCst)
    }

    /// Add a stub at a specific index
    pub fn add_stub(&self, stub: Stub, index: Option<usize>) {
        let mut stubs = self.stubs.write();
        let idx = index.unwrap_or(stubs.len());
        let idx = idx.min(stubs.len());
        stubs.insert(idx, stub);
    }

    /// Replace a stub at a specific index
    pub fn replace_stub(&self, index: usize, stub: Stub) -> Result<(), String> {
        let mut stubs = self.stubs.write();
        if index >= stubs.len() {
            return Err(format!("Stub index {index} out of bounds"));
        }
        stubs[index] = stub;
        Ok(())
    }

    /// Delete a stub at a specific index
    pub fn delete_stub(&self, index: usize) -> Result<(), String> {
        let mut stubs = self.stubs.write();
        if index >= stubs.len() {
            return Err(format!("Stub index {index} out of bounds"));
        }
        stubs.remove(index);
        Ok(())
    }

    /// Get all stubs
    pub fn get_stubs(&self) -> Vec<Stub> {
        self.stubs.read().clone()
    }

    /// Set enabled state
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

/// Error types for imposter management
#[derive(Debug, thiserror::Error)]
pub enum ImposterError {
    #[error("Port {0} is already in use")]
    PortInUse(u16),
    #[error("Imposter not found on port {0}")]
    NotFound(u16),
    #[error("Failed to bind port {0}: {1}")]
    BindError(u16, String),
    #[error("Invalid protocol: {0}")]
    InvalidProtocol(String),
    #[error("Stub index {0} out of bounds")]
    StubIndexOutOfBounds(usize),
}

/// Manages the lifecycle of multiple imposters
pub struct ImposterManager {
    /// Active imposters by port
    imposters: RwLock<HashMap<u16, Arc<Imposter>>>,
    /// Global shutdown signal (for future graceful shutdown)
    #[allow(dead_code)]
    shutdown_tx: broadcast::Sender<()>,
}

impl ImposterManager {
    /// Create a new imposter manager
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        Self {
            imposters: RwLock::new(HashMap::new()),
            shutdown_tx,
        }
    }

    /// Create and start an imposter
    /// Returns the assigned port (which may have been auto-assigned if not specified)
    pub async fn create_imposter(&self, config: ImposterConfig) -> Result<u16, ImposterError> {
        // Validate protocol first
        match config.protocol.as_str() {
            "http" | "https" => {}
            proto => return Err(ImposterError::InvalidProtocol(proto.to_string())),
        }

        // Determine port - either from config or auto-assign
        let port = if let Some(p) = config.port {
            // Check if specified port is already in use
            let imposters = self.imposters.read();
            if imposters.contains_key(&p) {
                return Err(ImposterError::PortInUse(p));
            }
            p
        } else {
            // Auto-assign port: find an available port starting from a base
            self.find_available_port().await?
        };

        // Create config with resolved port
        let mut resolved_config = config;
        resolved_config.port = Some(port);

        // Create imposter
        let mut imposter = Imposter::new(resolved_config);

        // Create shutdown channel for this imposter
        let (shutdown_tx, _) = broadcast::channel(1);
        imposter.shutdown_tx = Some(shutdown_tx.clone());

        let imposter = Arc::new(imposter);

        // Bind to port
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| ImposterError::BindError(port, e.to_string()))?;

        info!("Imposter bound to port {}", port);

        // Start serving
        let imposter_clone = Arc::clone(&imposter);
        let mut shutdown_rx = shutdown_tx.subscribe();

        let _handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let imposter = Arc::clone(&imposter_clone);
                                tokio::spawn(async move {
                                    let io = TokioIo::new(stream);
                                    let service = service_fn(move |req| {
                                        let imposter = Arc::clone(&imposter);
                                        async move {
                                            handle_imposter_request(req, imposter, addr).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        debug!("Connection error on port {}: {}", port, e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept error on port {}: {}", port, e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Imposter on port {} shutting down", port);
                        break;
                    }
                }
            }
        });

        // Store task handle (we need to work around the Arc)
        // Since we can't modify the Arc'd imposter, we'll track handles separately

        // Store imposter
        {
            let mut imposters = self.imposters.write();
            imposters.insert(port, imposter);
        }

        Ok(port)
    }

    /// Find an available port for auto-assignment
    /// Starts from port 49152 (start of dynamic/private port range) and finds first available
    async fn find_available_port(&self) -> Result<u16, ImposterError> {
        let existing_ports: std::collections::HashSet<u16> = {
            let imposters = self.imposters.read();
            imposters.keys().copied().collect()
        };

        // Start from dynamic port range (49152-65535)
        // Try ports in this range until we find one that's available
        for port in 49152..=65535u16 {
            if existing_ports.contains(&port) {
                continue;
            }
            // Try to bind to check if OS has it available
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    // Port is available, drop the listener and return
                    drop(listener);
                    return Ok(port);
                }
                Err(_) => continue, // Port in use by OS, try next
            }
        }

        Err(ImposterError::BindError(
            0,
            "No available ports in range 49152-65535".to_string(),
        ))
    }

    /// Delete an imposter
    pub async fn delete_imposter(&self, port: u16) -> Result<ImposterConfig, ImposterError> {
        let imposter = {
            let mut imposters = self.imposters.write();
            imposters
                .remove(&port)
                .ok_or(ImposterError::NotFound(port))?
        };

        // Send shutdown signal
        if let Some(ref tx) = imposter.shutdown_tx {
            let _ = tx.send(());
        }

        // Clear JavaScript inject state for this imposter
        #[cfg(feature = "javascript")]
        crate::scripting::clear_imposter_state(port);

        info!("Imposter on port {} deleted", port);
        Ok(imposter.config.clone())
    }

    /// Get an imposter by port
    pub fn get_imposter(&self, port: u16) -> Result<Arc<Imposter>, ImposterError> {
        let imposters = self.imposters.read();
        imposters
            .get(&port)
            .cloned()
            .ok_or(ImposterError::NotFound(port))
    }

    /// List all imposters
    pub fn list_imposters(&self) -> Vec<Arc<Imposter>> {
        let imposters = self.imposters.read();
        imposters.values().cloned().collect()
    }

    /// Delete all imposters
    pub async fn delete_all(&self) -> Vec<ImposterConfig> {
        let ports: Vec<u16> = {
            let imposters = self.imposters.read();
            imposters.keys().copied().collect()
        };

        let mut configs = Vec::new();
        for port in ports {
            if let Ok(config) = self.delete_imposter(port).await {
                configs.push(config);
            }
        }

        configs
    }

    /// Get imposter count (for future metrics)
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.imposters.read().len()
    }

    /// Add stub to an imposter
    pub fn add_stub(
        &self,
        port: u16,
        stub: Stub,
        index: Option<usize>,
    ) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter.add_stub(stub, index);
        Ok(())
    }

    /// Replace a stub
    pub fn replace_stub(&self, port: u16, index: usize, stub: Stub) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter
            .replace_stub(index, stub)
            .map_err(|_| ImposterError::StubIndexOutOfBounds(index))
    }

    /// Delete a stub
    pub fn delete_stub(&self, port: u16, index: usize) -> Result<(), ImposterError> {
        let imposter = self.get_imposter(port)?;
        imposter
            .delete_stub(index)
            .map_err(|_| ImposterError::StubIndexOutOfBounds(index))
    }

    /// Shutdown all imposters (for future graceful shutdown)
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        self.delete_all().await;
    }
}

impl Default for ImposterManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle a request to an imposter
async fn handle_imposter_request(
    req: Request<Incoming>,
    imposter: Arc<Imposter>,
    client_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Check if enabled
    if !imposter.is_enabled() {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("x-rift-imposter-disabled", "true")
            .body(Full::new(Bytes::from(
                r#"{"error": "Imposter is disabled"}"#,
            )))
            .unwrap());
    }

    // Increment request count
    imposter.increment_request_count();

    // Extract parts we need before consuming the request body
    let method = req.method().to_string();
    let uri = req.uri().clone();
    let headers_clone: HashMap<String, String> = req
        .headers()
        .iter()
        .map(|(k, v)| {
            // Preserve original header case by capitalizing like Mountebank does
            let key = k.as_str().to_string();
            let capitalized_key = key
                .split('-')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().chain(chars).collect(),
                    }
                })
                .collect::<Vec<_>>()
                .join("-");
            (capitalized_key, v.to_str().unwrap_or("").to_string())
        })
        .collect();
    let path = uri.path().to_string();
    let query_str = uri.query().unwrap_or("").to_string();

    // Always collect request body - needed for recording, copy behaviors, and predicate matching
    let body_string = match req.into_body().collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            if bytes.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&bytes).to_string())
            }
        }
        Err(_) => None,
    };

    // Build HeaderMap from captured headers for request context
    let mut headers_for_context = hyper::HeaderMap::new();
    for (k, v) in &headers_clone {
        if let (Ok(name), Ok(value)) = (
            hyper::header::HeaderName::from_bytes(k.as_bytes()),
            hyper::header::HeaderValue::from_str(v),
        ) {
            headers_for_context.insert(name, value);
        }
    }

    // Build request context for behaviors
    let request_context =
        RequestContext::from_request(&method, &uri, &headers_for_context, body_string.as_deref());

    // Record request if enabled
    if imposter.config.record_requests {
        let recorded = RecordedRequest {
            request_from: client_addr.to_string(),
            method: method.clone(),
            path: path.clone(),
            query: parse_query_string(&query_str),
            headers: headers_clone.clone(),
            body: body_string.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        imposter.record_request(&recorded);
    }

    // Find matching stub
    let method_str = method.as_str();
    let path_str = path.as_str();
    let query_opt = if query_str.is_empty() {
        None
    } else {
        Some(query_str.as_str())
    };

    if let Some((stub, stub_index)) =
        imposter.find_matching_stub(method_str, path_str, &headers_for_context, query_opt, body_string.as_deref())
    {
        // Check if this is a proxy response
        if let Some(proxy_config) = imposter.get_proxy_response(&stub, stub_index) {
            debug!("Handling proxy request to {}", proxy_config.to);
            match imposter
                .handle_proxy_request(
                    &proxy_config,
                    method_str,
                    &uri,
                    &headers_clone,
                    body_string.as_deref(),
                    stub_index,
                )
                .await
            {
                Ok((status, response_headers, body, latency)) => {
                    // Advance the cycler for this proxy response
                    imposter.advance_cycler_for_proxy(&stub, stub_index);

                    let mut response = Response::builder().status(status);

                    for (k, v) in &response_headers {
                        // Skip hop-by-hop headers
                        let k_lower = k.to_lowercase();
                        if k_lower != "transfer-encoding"
                            && k_lower != "connection"
                            && k_lower != "keep-alive"
                        {
                            response = response.header(k, v);
                        }
                    }

                    response = response.header("x-rift-imposter", "true");
                    response = response.header("x-rift-proxy", "true");

                    if let Some(ms) = latency {
                        response = response.header("x-rift-proxy-latency", ms.to_string());
                    }

                    return Ok(response.body(Full::new(Bytes::from(body))).unwrap());
                }
                Err(e) => {
                    warn!("Proxy request failed: {}", e);
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .header("x-rift-imposter", "true")
                        .header("x-rift-proxy-error", "true")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error": "Proxy error: {e}"}}"#
                        ))))
                        .unwrap());
                }
            }
        }

        // Check if this is an inject response (JavaScript function)
        #[cfg(feature = "javascript")]
        if let Some(inject_fn) = imposter.get_inject_response(&stub, stub_index) {
            debug!("Handling inject response");

            // Build request for inject function
            let mb_request = MountebankRequest {
                method: method.clone(),
                path: path.clone(),
                query: parse_query_string(&query_str),
                headers: headers_clone.clone(),
                body: body_string.clone(),
            };

            match execute_mountebank_inject(&inject_fn, &mb_request, imposter.config.port.unwrap_or(0)) {
                Ok(inject_response) => {
                    // Advance the cycler for this inject response
                    imposter.advance_cycler_for_inject(&stub, stub_index);

                    let mut response = Response::builder().status(inject_response.status_code);

                    for (k, v) in &inject_response.headers {
                        response = response.header(k, v);
                    }

                    response = response.header("x-rift-imposter", "true");
                    response = response.header("x-rift-inject", "true");

                    return Ok(response
                        .body(Full::new(Bytes::from(inject_response.body)))
                        .unwrap());
                }
                Err(e) => {
                    warn!("Inject function failed: {}", e);
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("x-rift-imposter", "true")
                        .header("x-rift-inject-error", "true")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error": "Inject error: {e}"}}"#
                        ))))
                        .unwrap());
                }
            }
        }

        if let Some((mut status, mut headers, mut body, behaviors, is_fault)) =
            imposter.execute_stub(&stub, stub_index)
        {
            // Handle faults - simulate connection errors
            if is_fault {
                match body.as_str() {
                    "CONNECTION_RESET_BY_PEER" => {
                        // Return empty response to simulate connection reset
                        // In real Mountebank, this would actually reset the TCP connection
                        return Ok(Response::builder()
                            .status(StatusCode::BAD_GATEWAY)
                            .header("x-rift-fault", "CONNECTION_RESET_BY_PEER")
                            .body(Full::new(Bytes::new()))
                            .unwrap());
                    }
                    "RANDOM_DATA_THEN_CLOSE" => {
                        return Ok(Response::builder()
                            .status(StatusCode::BAD_GATEWAY)
                            .header("x-rift-fault", "RANDOM_DATA_THEN_CLOSE")
                            .body(Full::new(Bytes::from_static(b"\x00\xff\xfe\xfd")))
                            .unwrap());
                    }
                    _ => {
                        return Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("x-rift-fault", &body)
                            .body(Full::new(Bytes::from(format!("Unknown fault: {body}"))))
                            .unwrap());
                    }
                }
            }

            // Apply behaviors if present
            if let Some(ref behaviors_json) = behaviors {
                // Parse behaviors
                if let Ok(parsed_behaviors) =
                    serde_json::from_value::<ResponseBehaviors>(behaviors_json.clone())
                {
                    // Apply wait behavior
                    if let Some(ref wait) = parsed_behaviors.wait {
                        let wait_ms = wait.get_duration_ms();
                        if wait_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                        }
                    }

                    // Apply copy behaviors
                    if !parsed_behaviors.copy.is_empty() {
                        body = apply_copy_behaviors(
                            &body,
                            &mut headers,
                            &parsed_behaviors.copy,
                            &request_context,
                        );
                    }

                    // Apply decorate behavior
                    if let Some(ref decorate_script) = parsed_behaviors.decorate {
                        // Handle JavaScript-style decorate (Mountebank format)
                        // Convert to Rhai or execute as JS
                        match apply_js_or_rhai_decorate(
                            decorate_script,
                            &request_context,
                            &body,
                            status,
                            &mut headers,
                        ) {
                            Ok((new_body, new_status)) => {
                                body = new_body;
                                status = new_status;
                            }
                            Err(e) => {
                                warn!("Decorate script error: {}", e);
                            }
                        }
                    }
                }
            }

            let mut response = Response::builder().status(status);

            for (k, v) in &headers {
                response = response.header(k, v);
            }

            response = response.header("x-rift-imposter", "true");

            return Ok(response.body(Full::new(Bytes::from(body))).unwrap());
        }
    }

    // No matching rule - return default response or 404
    if let Some(ref default) = imposter.config.default_response {
        let body = default
            .body
            .as_ref()
            .map(|b| {
                if b.is_string() {
                    b.as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string(b).unwrap_or_default()
                }
            })
            .unwrap_or_default();

        let mut response = Response::builder().status(default.status_code);
        for (k, v) in &default.headers {
            response = response.header(k, v);
        }
        response = response.header("x-rift-imposter", "true");
        response = response.header("x-rift-default-response", "true");

        return Ok(response.body(Full::new(Bytes::from(body))).unwrap());
    }

    // No match and no default - Mountebank returns 200 with empty body
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("x-rift-imposter", "true")
        .header("x-rift-no-match", "true")
        .body(Full::new(Bytes::new()))
        .unwrap())
}

/// Apply decorate behavior - handles both JavaScript and Rhai scripts
fn apply_js_or_rhai_decorate(
    script: &str,
    request: &RequestContext,
    body: &str,
    status: u16,
    headers: &mut HashMap<String, String>,
) -> Result<(String, u16), String> {
    // Check if it's a JavaScript function declaration
    if script.trim().starts_with("function") {
        #[cfg(feature = "javascript")]
        {
            // Use the JavaScript engine for proper execution
            let mb_request = crate::scripting::MountebankRequest {
                method: request.method.clone(),
                path: request.path.clone(),
                query: request.query.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
            };

            match crate::scripting::execute_mountebank_decorate(
                script,
                &mb_request,
                body,
                status,
                headers,
            ) {
                Ok(result) => {
                    // Update headers from the result
                    for (k, v) in result.headers {
                        headers.insert(k, v);
                    }
                    Ok((result.body, result.status_code))
                }
                Err(e) => Err(format!("JavaScript decorate error: {e}")),
            }
        }

        #[cfg(not(feature = "javascript"))]
        {
            // Fallback to Rhai conversion when JavaScript feature is disabled
            if let Some(start) = script.find('{') {
                if let Some(end) = script.rfind('}') {
                    let js_body = script[start + 1..end].trim();
                    let rhai_script = js_body.replace('\'', "\"");
                    return apply_decorate(&rhai_script, request, body, status, headers);
                }
            }
            Err("Could not parse JavaScript decorate function".to_string())
        }
    } else {
        // Assume it's Rhai script
        apply_decorate(script, request, body, status, headers)
    }
}

/// Parse query string into HashMap
fn parse_query_string(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
                },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Should match
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "get", "/test", None, &empty_headers, None)); // case-insensitive method

        // Should not match
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/other", None, &empty_headers, None));
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
        };
        let imposter = Imposter::new(config);

        let stub = Stub {
            predicates: vec![],
            responses: vec![StubResponse::Is {
                is: IsResponse {
                    status_code: 201,
                    headers: HashMap::new(),
                    body: Some(serde_json::json!({"created": true})),
                },
                behaviors: None,
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
        if let StubResponse::Is { is, behaviors } = response {
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

        if let StubResponse::Is { is, behaviors } = &config.stubs[0].responses[0] {
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
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "endsWith": {"path": "-details"}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: None,
                },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Should match
        assert!(Imposter::stub_matches(&stub, "GET", "/api/lender-details", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/user-details", None, &empty_headers, None));

        // Should not match
        assert!(!Imposter::stub_matches(&stub, "GET", "/details/other", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/api/details/v1", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_deep_equals_method() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "deepEquals": {"method": "GET"}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "get", "/test", None, &empty_headers, None)); // case-insensitive
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_deep_equals_body() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "deepEquals": {"body": ""}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Empty body should match
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, Some("")));
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));

        // Non-empty body should not match
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, Some("content")));
    }

    #[test]
    fn test_predicate_deep_equals_path() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "deepEquals": {"path": "/kaizen/auto/financing/lender-information/lenders"}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/kaizen/auto/financing/lender-information/lenders", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/other/path", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_contains_query() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "contains": {"query": {"lenderIds": "CofTest"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Should match - query contains "CofTest"
        assert!(Imposter::stub_matches(&stub, "GET", "/test", Some("lenderIds=CofTestWL"), &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/test", Some("lenderIds=CofTest"), &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/test", Some("lenderIds=123CofTest456"), &empty_headers, None));

        // Should not match
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", Some("lenderIds=Other"), &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_equals_headers() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "equals": {"headers": {"Content-Type": "application/json"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &headers, None));

        // Header key lookup is case-insensitive
        let mut headers_lower = HashMap::new();
        headers_lower.insert("content-type".to_string(), "application/json".to_string());
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &headers_lower, None));

        // Wrong value
        let mut wrong_headers = HashMap::new();
        wrong_headers.insert("Content-Type".to_string(), "text/html".to_string());
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", None, &wrong_headers, None));

        // Missing header
        let empty_headers = HashMap::new();
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_equals_body() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "equals": {"body": "{\"key\": \"value\"}"}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, Some("{\"key\": \"value\"}")));
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, Some("{\"other\": \"data\"}")));
    }

    #[test]
    fn test_predicate_exists() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "exists": {
                    "query": {"token": true},
                    "headers": {"Authorization": true},
                    "body": true
                }
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer xyz".to_string());

        // All exist
        assert!(Imposter::stub_matches(&stub, "POST", "/test", Some("token=abc"), &headers, Some("body content")));

        // Missing query param
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &headers, Some("body content")));

        // Missing header
        let empty_headers = HashMap::new();
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", Some("token=abc"), &empty_headers, Some("body content")));

        // Missing body
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", Some("token=abc"), &headers, None));
    }

    #[test]
    fn test_predicate_exists_false() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "exists": {"query": {"debug": false}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // debug param should NOT exist
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/test", Some("other=value"), &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/test", Some("debug=true"), &empty_headers, None));
    }

    #[test]
    fn test_predicate_logical_not() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "not": {"equals": {"method": "DELETE"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Should match anything except DELETE
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "DELETE", "/test", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_logical_or() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "or": [
                    {"equals": {"method": "GET"}},
                    {"equals": {"method": "HEAD"}}
                ]
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/test", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "HEAD", "/test", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_logical_and() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "and": [
                    {"equals": {"method": "GET"}},
                    {"startsWith": {"path": "/api"}}
                ]
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/api/users", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "POST", "/api/users", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/other", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_matches_regex_all_fields() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "matches": {
                    "path": "^/api/v[0-9]+/",
                    "method": "^(GET|POST)$"
                }
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/api/v1/users", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "POST", "/api/v2/items", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "DELETE", "/api/v1/users", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/other/path", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_matches_body_regex() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "matches": {"body": "\"userId\":\\s*\"[a-f0-9-]+\""}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, Some(r#"{"userId": "abc-123-def"}"#)));
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None, &empty_headers, Some(r#"{"userId": "invalid!"}"#)));
    }

    #[test]
    fn test_predicate_case_sensitive() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "equals": {"path": "/API/Users"},
                "caseSensitive": true
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/API/Users", None, &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/api/users", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_except_pattern() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "equals": {"path": "/api/users"},
                "except": "\\?.*$"  // Strip query string before matching
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/api/users", None, &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/api/users?page=1", None, &empty_headers, None));
    }

    #[test]
    fn test_predicate_complex_mountebank_format() {
        // Test the exact format from user's JSON
        let stub = Stub {
            predicates: vec![
                serde_json::json!({
                    "endsWith": {"path": "lender-details"}
                }),
                serde_json::json!({
                    "contains": {"query": {"lenderIds": "ALL"}}
                }),
                serde_json::json!({
                    "deepEquals": {"method": "GET"}
                }),
            ],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: Some("LenderDetails-v1-lenders_AllLenderDetails".to_string()),
        };

        let empty_headers = HashMap::new();

        // Should match
        assert!(Imposter::stub_matches(
            &stub,
            "GET",
            "/kaizen/auto/financing/lender-information/lender-details",
            Some("lenderIds=ALL"),
            &empty_headers,
            None
        ));

        // Should not match - wrong method
        assert!(!Imposter::stub_matches(
            &stub,
            "POST",
            "/kaizen/auto/financing/lender-information/lender-details",
            Some("lenderIds=ALL"),
            &empty_headers,
            None
        ));

        // Should not match - path doesn't end with lender-details
        assert!(!Imposter::stub_matches(
            &stub,
            "GET",
            "/kaizen/auto/financing/lender-information/lenders",
            Some("lenderIds=ALL"),
            &empty_headers,
            None
        ));

        // Should not match - query doesn't contain ALL
        assert!(!Imposter::stub_matches(
            &stub,
            "GET",
            "/kaizen/auto/financing/lender-information/lender-details",
            Some("lenderIds=LENDER1"),
            &empty_headers,
            None
        ));
    }

    // =============================================================================
    // Real-World JSON Format Parsing Tests
    // =============================================================================

    #[test]
    fn test_parse_real_world_imposter_json() {
        // Test parsing of a complete real-world imposter JSON with all alternative formats
        let json = r#"{
            "allowCORS": true,
            "protocol": "http",
            "port": 8201,
            "stubs": [{
                "scenarioName": "LenderDetails-v1-lenders_Lender1",
                "predicates": [
                    {"equals": {"query": {"lenderIds": "LENDER1"}}},
                    {"deepEquals": {"method": "GET"}}
                ],
                "responses": [{
                    "behaviors": [{"wait": " function() { var min = Math.ceil(0); var max = Math.floor(0); return min; } "}],
                    "is": {
                        "statusCode": "200",
                        "headers": {"Accept": "application/json", "Content-Type": "application/json"},
                        "body": "{\"lenders\": [{\"lenderId\": \"111111\"}]}"
                    },
                    "proxy": null
                }]
            }],
            "service_name": "LenderDetails_v1_lenders",
            "service_info": {
                "virtualServiceInfo": {
                    "serviceName": "LenderDetails-v1-lenders",
                    "realEndpoint": "https://api.example.com/lenders"
                }
            }
        }"#;

        let config: ImposterConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, Some(8201));
        assert_eq!(config.protocol, "http");
        assert!(config.allow_cors);
        assert_eq!(config.service_name, Some("LenderDetails_v1_lenders".to_string()));
        assert!(config.service_info.is_some());
        assert_eq!(config.stubs.len(), 1);

        let stub = &config.stubs[0];
        assert_eq!(stub.scenario_name, Some("LenderDetails-v1-lenders_Lender1".to_string()));
        assert_eq!(stub.predicates.len(), 2);

        if let StubResponse::Is { is, behaviors } = &stub.responses[0] {
            assert_eq!(is.status_code, 200);
            assert!(behaviors.is_some());
            assert!(is.body.is_some());
        } else {
            panic!("Expected Is response");
        }
    }

    #[test]
    fn test_parse_proxy_response_with_mode() {
        let json = r#"{
            "port": 4545,
            "protocol": "http",
            "stubs": [{
                "predicates": [{"equals": {"path": "/redirect"}}],
                "responses": [{
                    "proxy": {
                        "to": "https://api.example.com",
                        "mode": "proxyTransparent"
                    }
                }]
            }]
        }"#;

        let config: ImposterConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.stubs.len(), 1);

        if let StubResponse::Proxy { proxy } = &config.stubs[0].responses[0] {
            assert_eq!(proxy.to, "https://api.example.com");
            assert_eq!(proxy.mode, "proxyTransparent");
        } else {
            panic!("Expected Proxy response");
        }
    }

    #[test]
    fn test_parse_mixed_responses_with_proxy_only_stub() {
        // Test that we can have both is responses and proxy responses in different stubs
        let json = r#"{
            "port": 4545,
            "protocol": "http",
            "stubs": [
                {
                    "predicates": [{"equals": {"path": "/local"}}],
                    "responses": [{
                        "behaviors": [{"wait": 0}],
                        "is": {"statusCode": "200", "body": "local"},
                        "proxy": null
                    }]
                },
                {
                    "predicates": [{"equals": {"path": "/proxy"}}],
                    "responses": [{
                        "proxy": {
                            "to": "http://backend:8080",
                            "mode": "proxyTransparent"
                        }
                    }]
                }
            ]
        }"#;

        let config: ImposterConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.stubs.len(), 2);

        // First stub should be Is
        assert!(matches!(&config.stubs[0].responses[0], StubResponse::Is { .. }));

        // Second stub should be Proxy
        assert!(matches!(&config.stubs[1].responses[0], StubResponse::Proxy { .. }));
    }

    #[test]
    fn test_predicate_contains_in_headers() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "contains": {"headers": {"Authorization": "Bearer"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer abc123token".to_string());

        assert!(Imposter::stub_matches(&stub, "GET", "/", None, &headers, None));

        // Wrong token type
        let mut headers_basic = HashMap::new();
        headers_basic.insert("Authorization".to_string(), "Basic xyz".to_string());
        assert!(!Imposter::stub_matches(&stub, "GET", "/", None, &headers_basic, None));
    }

    #[test]
    fn test_predicate_starts_with_in_query() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "startsWith": {"query": {"filter": "status_"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/", Some("filter=status_active"), &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/", Some("filter=status_pending"), &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/", Some("filter=type_user"), &empty_headers, None));
    }

    #[test]
    fn test_predicate_ends_with_in_query() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "endsWith": {"query": {"filename": ".json"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        assert!(Imposter::stub_matches(&stub, "GET", "/", Some("filename=data.json"), &empty_headers, None));
        assert!(Imposter::stub_matches(&stub, "GET", "/", Some("filename=config.json"), &empty_headers, None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/", Some("filename=data.xml"), &empty_headers, None));
    }

    #[test]
    fn test_predicate_matches_regex_in_query() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "matches": {"query": {"id": "^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Valid UUID
        assert!(Imposter::stub_matches(&stub, "GET", "/", Some("id=550e8400-e29b-41d4-a716-446655440000"), &empty_headers, None));

        // Invalid UUID
        assert!(!Imposter::stub_matches(&stub, "GET", "/", Some("id=not-a-uuid"), &empty_headers, None));
    }

    #[test]
    fn test_predicate_matches_regex_in_headers() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "matches": {"headers": {"User-Agent": "Mozilla.*Firefox"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let mut firefox_headers = HashMap::new();
        firefox_headers.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; rv:91.0) Gecko/20100101 Firefox/91.0".to_string());

        assert!(Imposter::stub_matches(&stub, "GET", "/", None, &firefox_headers, None));

        let mut chrome_headers = HashMap::new();
        chrome_headers.insert("User-Agent".to_string(), "Mozilla/5.0 Chrome/96.0".to_string());

        assert!(!Imposter::stub_matches(&stub, "GET", "/", None, &chrome_headers, None));
    }

    #[test]
    fn test_predicate_deep_equals_headers() {
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "deepEquals": {"headers": {"Content-Type": "application/json"}}
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        // Exact match with only one header
        let mut exact_headers = HashMap::new();
        exact_headers.insert("Content-Type".to_string(), "application/json".to_string());

        assert!(Imposter::stub_matches(&stub, "POST", "/", None, &exact_headers, None));

        // Extra headers should fail deepEquals
        let mut extra_headers = HashMap::new();
        extra_headers.insert("Content-Type".to_string(), "application/json".to_string());
        extra_headers.insert("Accept".to_string(), "application/json".to_string());

        assert!(!Imposter::stub_matches(&stub, "POST", "/", None, &extra_headers, None));
    }

    #[test]
    fn test_complex_nested_logical_predicates() {
        // Complex: (GET OR POST) AND (/api/* path) AND NOT (/api/admin/*)
        let stub = Stub {
            predicates: vec![serde_json::json!({
                "and": [
                    {"or": [
                        {"equals": {"method": "GET"}},
                        {"equals": {"method": "POST"}}
                    ]},
                    {"startsWith": {"path": "/api/"}},
                    {"not": {"startsWith": {"path": "/api/admin/"}}}
                ]
            })],
            responses: vec![StubResponse::Is {
                is: IsResponse { status_code: 200, headers: HashMap::new(), body: None },
                behaviors: None,
            }],
            scenario_name: None,
        };

        let empty_headers = HashMap::new();

        // Should match: GET /api/users
        assert!(Imposter::stub_matches(&stub, "GET", "/api/users", None, &empty_headers, None));

        // Should match: POST /api/data
        assert!(Imposter::stub_matches(&stub, "POST", "/api/data", None, &empty_headers, None));

        // Should NOT match: DELETE /api/users
        assert!(!Imposter::stub_matches(&stub, "DELETE", "/api/users", None, &empty_headers, None));

        // Should NOT match: GET /api/admin/config
        assert!(!Imposter::stub_matches(&stub, "GET", "/api/admin/config", None, &empty_headers, None));

        // Should NOT match: GET /other/path
        assert!(!Imposter::stub_matches(&stub, "GET", "/other/path", None, &empty_headers, None));
    }

    #[test]
    fn test_wait_behavior_js_function_parsing() {
        // Test that the JS wait function from user's JSON is accepted
        let json = r#"{
            "wait": " function() { var min = Math.ceil(0); var max = Math.floor(0); var num = Math.floor(Math.random() * (max - min + 1)); var wait = (num + min); return wait; } "
        }"#;

        let behaviors: crate::behaviors::ResponseBehaviors = serde_json::from_str(json).unwrap();
        assert!(behaviors.wait.is_some());
    }

    #[test]
    fn test_serialization_roundtrip() {
        // Test that we can serialize and deserialize without losing data
        let original = ImposterConfig {
            port: Some(8080),
            protocol: "http".to_string(),
            name: Some("test".to_string()),
            record_requests: false,
            stubs: vec![Stub {
                predicates: vec![serde_json::json!({"equals": {"path": "/test"}})],
                responses: vec![StubResponse::Is {
                    is: IsResponse {
                        status_code: 200,
                        headers: HashMap::new(),
                        body: Some(serde_json::json!("test body")),
                    },
                    behaviors: Some(serde_json::json!({"wait": 100})),
                }],
                scenario_name: Some("test-scenario".to_string()),
            }],
            default_response: None,
            allow_cors: true,
            service_name: Some("test-service".to_string()),
            service_info: Some(serde_json::json!({"version": "1.0"})),
        };

        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ImposterConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.port, original.port);
        assert_eq!(deserialized.allow_cors, original.allow_cors);
        assert_eq!(deserialized.service_name, original.service_name);
        assert_eq!(deserialized.stubs.len(), 1);
        assert_eq!(deserialized.stubs[0].scenario_name, original.stubs[0].scenario_name);
    }
}

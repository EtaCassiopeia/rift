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
pub struct Stub {
    #[serde(default)]
    pub predicates: Vec<serde_json::Value>,
    pub responses: Vec<StubResponse>,
}

/// Response within a stub
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
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
    pub port: u16,
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
        _headers: &hyper::HeaderMap,
        query: Option<&str>,
    ) -> Option<(Stub, usize)> {
        let stubs = self.stubs.read();
        for (index, stub) in stubs.iter().enumerate() {
            if Self::stub_matches(stub, method, path, query) {
                return Some((stub.clone(), index));
            }
        }
        None
    }

    /// Check if a stub matches a request
    fn stub_matches(stub: &Stub, method: &str, path: &str, query: Option<&str>) -> bool {
        // If no predicates, match everything
        if stub.predicates.is_empty() {
            return true;
        }

        for predicate in &stub.predicates {
            if !Self::predicate_matches(predicate, method, path, query) {
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

    /// Check if a single predicate matches
    fn predicate_matches(
        predicate: &serde_json::Value,
        method: &str,
        path: &str,
        query: Option<&str>,
    ) -> bool {
        // Note: Mountebank compares raw URL-encoded paths, so we don't decode
        if let Some(obj) = predicate.as_object() {
            // Check if case-sensitive matching is requested
            // Note: Mountebank defaults to case-insensitive for path matching
            let case_sensitive = obj
                .get("caseSensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Get the except pattern for stripping values before matching
            let except_pattern = obj.get("except").and_then(|v| v.as_str());

            // Helper to apply except pattern to a value
            let apply_except = |value: &str| -> String {
                if let Some(pattern) = except_pattern {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        return re.replace_all(value, "").to_string();
                    }
                }
                value.to_string()
            };

            // Handle "equals" predicate
            if let Some(equals) = obj.get("equals").and_then(|v| v.as_object()) {
                if let Some(m) = equals.get("method").and_then(|v| v.as_str()) {
                    if !m.eq_ignore_ascii_case(method) {
                        return false;
                    }
                }
                if let Some(p) = equals.get("path").and_then(|v| v.as_str()) {
                    let actual_path = apply_except(path);
                    let matches = if case_sensitive {
                        p == actual_path
                    } else {
                        p.eq_ignore_ascii_case(&actual_path)
                    };
                    if !matches {
                        return false;
                    }
                }
                // Handle query matching for equals (subset match)
                if let Some(expected_query) = equals.get("query").and_then(|q| q.as_object()) {
                    let actual_query = Self::parse_query(query);
                    for (key, expected_val) in expected_query {
                        let expected_str = expected_val.as_str().unwrap_or("");
                        if let Some(actual_val) = actual_query.get(key) {
                            if actual_val != expected_str {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }
            }

            // Handle "deepEquals" predicate (exact match - no extra fields allowed)
            if let Some(deep_equals) = obj.get("deepEquals").and_then(|v| v.as_object()) {
                // Handle query matching for deepEquals (exact match)
                if let Some(expected_query) = deep_equals.get("query").and_then(|q| q.as_object()) {
                    let actual_query = Self::parse_query(query);

                    // Check counts match first
                    if expected_query.len() != actual_query.len() {
                        return false;
                    }

                    // Check all expected fields exist and match
                    for (key, expected_val) in expected_query {
                        let expected_str = expected_val.as_str().unwrap_or("");
                        if let Some(actual_val) = actual_query.get(key) {
                            if actual_val != expected_str {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }
            }

            // Handle "startsWith" predicate
            if let Some(starts) = obj.get("startsWith").and_then(|v| v.as_object()) {
                if let Some(p) = starts.get("path").and_then(|v| v.as_str()) {
                    let actual_path = apply_except(path);
                    if !actual_path.starts_with(p) {
                        return false;
                    }
                }
            }

            // Handle "contains" predicate
            if let Some(contains) = obj.get("contains").and_then(|v| v.as_object()) {
                if let Some(p) = contains.get("path").and_then(|v| v.as_str()) {
                    let actual_path = apply_except(path);
                    if !actual_path.contains(p) {
                        return false;
                    }
                }
            }

            // Handle "matches" predicate (regex)
            if let Some(matches) = obj.get("matches").and_then(|v| v.as_object()) {
                if let Some(p) = matches.get("path").and_then(|v| v.as_str()) {
                    if let Ok(re) = regex::Regex::new(p) {
                        let actual_path = apply_except(path);
                        if !re.is_match(&actual_path) {
                            return false;
                        }
                    }
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
    pub async fn create_imposter(&self, config: ImposterConfig) -> Result<(), ImposterError> {
        let port = config.port;

        // Check if port is already in use
        {
            let imposters = self.imposters.read();
            if imposters.contains_key(&port) {
                return Err(ImposterError::PortInUse(port));
            }
        }

        // Validate protocol
        match config.protocol.as_str() {
            "http" | "https" => {}
            proto => return Err(ImposterError::InvalidProtocol(proto.to_string())),
        }

        // Create imposter
        let mut imposter = Imposter::new(config);

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

        Ok(())
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
        imposter.find_matching_stub(method_str, path_str, &headers_for_context, query_opt)
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

            match execute_mountebank_inject(&inject_fn, &mb_request, imposter.config.port) {
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
        assert_eq!(config.port, 8080);
        assert_eq!(config.protocol, "http");
        assert!(!config.record_requests);
        assert!(config.stubs.is_empty());
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
        };

        // Should match
        assert!(Imposter::stub_matches(&stub, "GET", "/test", None));
        assert!(Imposter::stub_matches(&stub, "get", "/test", None)); // case-insensitive method

        // Should not match
        assert!(!Imposter::stub_matches(&stub, "POST", "/test", None));
        assert!(!Imposter::stub_matches(&stub, "GET", "/other", None));
    }

    #[test]
    fn test_execute_stub() {
        let config = ImposterConfig {
            port: 8080,
            protocol: "http".to_string(),
            name: Some("test".to_string()),
            record_requests: false,
            stubs: vec![],
            default_response: None,
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
            port: 19999,
            protocol: "http".to_string(),
            name: Some("test".to_string()),
            record_requests: false,
            stubs: vec![],
            default_response: None,
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
}

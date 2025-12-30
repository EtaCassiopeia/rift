//! Core Imposter struct and implementation.
//!
//! This module contains the Imposter struct which represents a single
//! running imposter instance with its configuration, stubs, and state.

use super::predicates::stub_matches;
use super::response::{
    create_response_preview, create_stub_from_proxy_response, execute_stub_response,
    execute_stub_response_with_rift, get_rift_script_config,
};
use super::types::{
    DebugImposter, DebugResponsePreview, DebugStubInfo, ImposterConfig, ProxyResponse,
    RecordedRequest, ResponseMode, RiftResponseExtension, RiftScriptConfig, Stub, StubResponse,
};
use crate::backends::InMemoryFlowStore;
use crate::behaviors::ResponseCycler;
use crate::extensions::flow_state::{FlowStore, NoOpFlowStore};
use crate::recording::{ProxyMode, RecordedResponse, RecordingStore, RequestSignature};
use anyhow::Context;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/// Runtime state of an imposter
pub struct Imposter {
    pub config: ImposterConfig,
    /// Mutable stubs (can be modified at runtime)
    pub stubs: RwLock<Vec<Stub>>,
    /// Response cycling state (for future use with response arrays)
    pub response_cycler: ResponseCycler,
    /// Recording store for proxy responses (for future proxy mode support)
    pub recording_store: Arc<RecordingStore>,
    /// Recorded requests (if record_requests is true)
    pub recorded_requests: RwLock<Vec<RecordedRequest>>,
    /// Request count
    pub request_count: AtomicU64,
    /// Whether imposter is enabled
    pub enabled: AtomicBool,
    /// Creation timestamp (for future metrics/admin display)
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Shutdown signal sender (for future graceful shutdown)
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    /// Flow store for Rift extensions (stateful scripting)
    pub flow_store: Arc<dyn FlowStore>,
}

impl Imposter {
    /// Create a new imposter from config
    pub fn new(config: ImposterConfig) -> Self {
        let stubs = config.stubs.clone();

        // Extract proxy mode from stubs (use first proxy response's mode)
        let proxy_mode = Self::extract_proxy_mode(&stubs);

        // Initialize flow store based on _rift.flowState configuration
        let flow_store = Self::create_flow_store(&config);

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
            flow_store,
        }
    }

    /// Create flow store based on _rift.flowState configuration
    fn create_flow_store(config: &ImposterConfig) -> Arc<dyn FlowStore> {
        let Some(ref rift_config) = config.rift else {
            return Arc::new(NoOpFlowStore);
        };

        let Some(ref flow_state_config) = rift_config.flow_state else {
            return Arc::new(NoOpFlowStore);
        };

        match flow_state_config.backend.as_str() {
            "inmemory" => {
                info!(
                    "Creating InMemory FlowStore for imposter (ttl={}s)",
                    flow_state_config.ttl_seconds
                );
                Arc::new(InMemoryFlowStore::new(flow_state_config.ttl_seconds as u64))
            }
            "redis" => Self::create_redis_flow_store(flow_state_config),
            other => {
                warn!("Unknown flow state backend '{}', using NoOp", other);
                Arc::new(NoOpFlowStore)
            }
        }
    }

    /// Create Redis flow store if configured and available
    #[allow(unused_variables)]
    fn create_redis_flow_store(
        flow_state_config: &crate::imposter::types::RiftFlowStateConfig,
    ) -> Arc<dyn FlowStore> {
        #[cfg(feature = "redis-backend")]
        {
            let Some(ref redis_config) = flow_state_config.redis else {
                error!("Redis backend selected but no redis config provided, falling back to NoOp");
                return Arc::new(NoOpFlowStore);
            };

            use crate::backends::RedisFlowStore;
            match RedisFlowStore::new(
                &redis_config.url,
                redis_config.pool_size,
                redis_config.key_prefix.clone(),
                flow_state_config.ttl_seconds,
            ) {
                Ok(store) => {
                    info!(
                        "Created Redis FlowStore for imposter (url={}, ttl={}s)",
                        redis_config.url, flow_state_config.ttl_seconds
                    );
                    Arc::new(store)
                }
                Err(e) => {
                    error!(
                        "Failed to create Redis FlowStore: {}, falling back to NoOp",
                        e
                    );
                    Arc::new(NoOpFlowStore)
                }
            }
        }

        #[cfg(not(feature = "redis-backend"))]
        {
            error!("Redis backend not available (compile with --features redis-backend), falling back to NoOp");
            Arc::new(NoOpFlowStore)
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
        // Call the extended version with no client info (backward compatible)
        self.find_matching_stub_with_client(method, path, headers, query, body, None, None)
    }

    /// Find a matching stub with client address information (for requestFrom/ip predicates)
    #[allow(clippy::too_many_arguments)]
    pub fn find_matching_stub_with_client(
        &self,
        method: &str,
        path: &str,
        headers: &hyper::HeaderMap,
        query: Option<&str>,
        body: Option<&str>,
        request_from: Option<&str>,
        client_ip: Option<&str>,
    ) -> Option<(Stub, usize)> {
        let stubs = self.stubs.read();
        let headers_map = Self::header_map_to_hashmap(headers);
        // Parse form data if Content-Type is application/x-www-form-urlencoded
        let form = Self::parse_form_data(headers, body);

        for (index, stub) in stubs.iter().enumerate() {
            if stub_matches(
                &stub.predicates,
                method,
                path,
                query,
                &headers_map,
                body,
                request_from,
                client_ip,
                form.as_ref(),
            ) {
                return Some((stub.clone(), index));
            }
        }
        None
    }

    /// Parse form-urlencoded data from body if Content-Type matches
    fn parse_form_data(
        headers: &hyper::HeaderMap,
        body: Option<&str>,
    ) -> Option<HashMap<String, String>> {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.contains("application/x-www-form-urlencoded") {
            if let Some(body_str) = body {
                return Some(
                    body_str
                        .split('&')
                        .filter(|s| !s.is_empty())
                        .filter_map(|pair| {
                            let mut parts = pair.splitn(2, '=');
                            let key = parts.next()?.to_string();
                            let value = parts
                                .next()
                                .map(|v| urlencoding::decode(v).unwrap_or_default().into_owned())
                                .unwrap_or_default();
                            Some((
                                urlencoding::decode(&key).unwrap_or_default().into_owned(),
                                value,
                            ))
                        })
                        .collect(),
                );
            }
        }
        None
    }

    /// Get all stubs info for debug purposes (Rift extension)
    pub fn get_all_stubs_info(&self) -> Vec<DebugStubInfo> {
        let stubs = self.stubs.read();
        stubs
            .iter()
            .enumerate()
            .map(|(index, stub)| DebugStubInfo {
                index,
                id: stub.id.clone(),
                predicates: stub.predicates.clone(),
                response_count: stub.responses.len(),
            })
            .collect()
    }

    /// Get imposter info for debug purposes (Rift extension)
    pub fn get_debug_imposter_info(&self) -> DebugImposter {
        let stubs = self.stubs.read();
        DebugImposter {
            port: self.config.port.unwrap_or(0),
            name: self.config.name.clone(),
            protocol: self.config.protocol.clone(),
            stub_count: stubs.len(),
        }
    }

    /// Create response preview from a stub (Rift extension)
    pub fn get_response_preview(&self, stub: &Stub, stub_index: usize) -> DebugResponsePreview {
        if stub.responses.is_empty() {
            return DebugResponsePreview {
                response_type: "unknown".to_string(),
                status_code: None,
                headers: None,
                body_preview: None,
            };
        }

        // Get the current response from the cycler
        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .peek_response_index(&rule_id, stub.responses.len());

        if let Some(response) = stub.responses.get(response_index) {
            return create_response_preview(response);
        }

        // Fallback to first response
        if let Some(response) = stub.responses.first() {
            return create_response_preview(response);
        }

        DebugResponsePreview {
            response_type: "unknown".to_string(),
            status_code: None,
            headers: None,
            body_preview: None,
        }
    }

    /// Convert hyper HeaderMap to HashMap<String, String>
    fn header_map_to_hashmap(headers: &hyper::HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect()
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
        execute_stub_response(response)
    }

    /// Execute a stub and get the response with behaviors and rift extensions
    /// Returns (status, headers, body, behaviors, rift_extension, response_mode, is_fault)
    #[allow(clippy::type_complexity)]
    pub fn execute_stub_with_rift(
        &self,
        stub: &Stub,
        stub_index: usize,
    ) -> Option<(
        u16,
        HashMap<String, String>,
        String,
        Option<serde_json::Value>,
        Option<RiftResponseExtension>,
        ResponseMode,
        bool,
    )> {
        if stub.responses.is_empty() {
            return None;
        }

        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .get_response_index_with_per_response_repeat(&rule_id, &stub.responses);

        let response = stub.responses.get(response_index)?;
        execute_stub_response_with_rift(response)
    }

    /// Get RiftScript response if present
    pub fn get_rift_script_response(
        &self,
        stub: &Stub,
        stub_index: usize,
    ) -> Option<RiftScriptConfig> {
        if stub.responses.is_empty() {
            return None;
        }

        let rule_id = format!("stub_{stub_index}");
        let response_index = self
            .response_cycler
            .peek_response_index(&rule_id, stub.responses.len());

        let response = stub.responses.get(response_index)?;
        get_rift_script_config(response)
    }

    /// Advance cycler for RiftScript response
    pub fn advance_cycler_for_rift_script(&self, stub: &Stub, stub_index: usize) {
        let rule_id = format!("stub_{stub_index}");
        self.response_cycler
            .get_response_index_with_per_response_repeat(&rule_id, &stub.responses);
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
    // Used with javascript feature
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
    // Used with javascript feature
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

    /// Insert a generated stub at the specified index
    pub fn insert_generated_stub(&self, stub: Stub, before_index: usize) {
        let mut stubs = self.stubs.write();
        let index = before_index.min(stubs.len());
        stubs.insert(index, stub);
        debug!("Inserted generated stub at index {}", index);
    }

    /// Insert or append a generated stub based on proxy mode
    /// For proxyOnce: Insert new stub BEFORE the proxy stub (so it matches first next time)
    /// For proxyAlways: Append response to existing stub AFTER proxy stub, or insert new AFTER proxy
    pub fn insert_or_append_proxy_stub(
        &self,
        stub: Stub,
        proxy_stub_index: usize,
        proxy_mode: &str,
    ) {
        let mut stubs = self.stubs.write();

        if proxy_mode == "proxyAlways" {
            // For proxyAlways, recorded stubs go AFTER the proxy stub
            // This ensures proxy always runs first and records each request

            // Try to find existing stub with matching predicates (after the proxy stub)
            let matching_stub_idx = stubs
                .iter()
                .enumerate()
                .skip(proxy_stub_index + 1) // Only look after the proxy stub
                .find(|(_, existing)| {
                    // Compare predicates (JSON comparison)
                    let existing_preds =
                        serde_json::to_string(&existing.predicates).unwrap_or_default();
                    let new_preds = serde_json::to_string(&stub.predicates).unwrap_or_default();
                    existing_preds == new_preds && !existing.predicates.is_empty()
                })
                .map(|(idx, _)| idx);

            if let Some(idx) = matching_stub_idx {
                // Append responses to existing stub
                for response in stub.responses {
                    stubs[idx].responses.push(response);
                }
                debug!(
                    "Appended response to existing stub at index {} (proxyAlways mode, {} total responses)",
                    idx,
                    stubs[idx].responses.len()
                );
                return;
            }

            // No matching stub found: insert new stub AFTER the proxy stub
            let insert_index = (proxy_stub_index + 1).min(stubs.len());
            stubs.insert(insert_index, stub);
            debug!(
                "Inserted generated stub at index {} after proxy (proxyAlways mode)",
                insert_index
            );
        } else {
            // For proxyOnce: insert new stub BEFORE the proxy stub
            // This ensures the recorded stub matches first on subsequent requests
            let index = proxy_stub_index.min(stubs.len());
            stubs.insert(index, stub);
            debug!(
                "Inserted generated stub at index {} before proxy (proxyOnce mode)",
                index
            );
        }
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
    ) -> anyhow::Result<(u16, HashMap<String, String>, String, Option<u64>)> {
        let client = get_http_client();

        info!("Proxy config - addDecorateBehavior: {:?}, addWaitBehavior: {}, predicateGenerators: {:?}",
            proxy_config.add_decorate_behavior, proxy_config.add_wait_behavior, proxy_config.predicate_generators);

        // Build the proxy URL, applying path rewrite if configured
        let original_path = uri.path();
        let rewritten_path = if let Some(ref rewrite) = proxy_config.path_rewrite {
            original_path.replacen(&rewrite.from, &rewrite.to, 1)
        } else {
            original_path.to_string()
        };

        let target_url = format!(
            "{}{}{}",
            proxy_config.to,
            rewritten_path,
            uri.query().map(|q| format!("?{q}")).unwrap_or_default()
        );

        if proxy_config.path_rewrite.is_some() {
            debug!(
                "Proxy request to: {} (path rewritten from '{}')",
                target_url, original_path
            );
        } else {
            debug!("Proxy request to: {}", target_url);
        }

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
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to send proxy request to {}", target_url))?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let status = response.status().as_u16();
        let response_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body_bytes = response
            .bytes()
            .await
            .with_context(|| format!("Failed to read response body from {}", target_url))?;
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

            let new_stub = create_stub_from_proxy_response(
                predicates,
                status,
                &response_headers,
                &body_str,
                latency_for_stub,
                proxy_config.add_decorate_behavior.clone(),
            );

            // Insert or append the stub based on proxy mode
            // proxyOnce: Insert new stub before the proxy stub
            // proxyAlways: Append response to existing stub with matching predicates
            let mode = if proxy_config.mode.is_empty() {
                "proxyOnce"
            } else {
                &proxy_config.mode
            };
            self.insert_or_append_proxy_stub(new_stub, stub_index, mode);
            debug!(
                "Generated stub from proxy response for path {} (mode: {})",
                uri.path(),
                mode
            );
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

    /// Get a specific stub by index
    pub fn get_stub(&self, index: usize) -> Option<Stub> {
        let stubs = self.stubs.read();
        stubs.get(index).cloned()
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

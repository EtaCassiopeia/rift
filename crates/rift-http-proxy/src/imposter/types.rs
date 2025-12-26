//! Type definitions for Mountebank-compatible imposter management.
//!
//! This module contains all the structs, enums, and type aliases used by the imposter system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Recorded Request Types
// ============================================================================

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

// ============================================================================
// Debug Mode Structures (Rift Extension)
// ============================================================================

/// Debug response for X-Rift-Debug header (Rift extension)
/// Returns match information instead of executing the response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugResponse {
    pub debug: bool,
    pub request: DebugRequest,
    pub imposter: DebugImposter,
    pub match_result: DebugMatchResult,
}

/// Debug request information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugRequest {
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Debug imposter information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugImposter {
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub protocol: String,
    pub stub_count: usize,
}

/// Debug match result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugMatchResult {
    pub matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stub_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stub_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicates: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_preview: Option<DebugResponsePreview>,
    /// All stubs for inspection when no match found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_stubs: Option<Vec<DebugStubInfo>>,
    /// Reason for no match
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Debug response preview (subset of actual response)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugResponsePreview {
    pub response_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    /// Truncated body preview (first 500 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

/// Debug stub info for listing all stubs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugStubInfo {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub predicates: Vec<serde_json::Value>,
    pub response_count: usize,
}

// ============================================================================
// Stub Types
// ============================================================================

/// Stub definition (Mountebank-compatible with Rift extensions)
/// Field ordering matches Mountebank output: scenarioName, predicates, responses, _links
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stub {
    /// Optional scenario name for documentation/organization (Mountebank compatible)
    /// Placed first to match Mountebank output ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_name: Option<String>,
    /// Optional unique identifier for the stub (Rift extension)
    /// Useful for targeting specific stubs for updates/deletion without relying on index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default)]
    pub predicates: Vec<serde_json::Value>,
    pub responses: Vec<StubResponse>,
}

/// Response within a stub - wrapper type that handles various formats
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "StubResponseRaw", into = "StubResponseOut")]
pub enum StubResponse {
    Is {
        is: IsResponse,
        #[serde(rename = "_behaviors", skip_serializing_if = "Option::is_none")]
        behaviors: Option<serde_json::Value>,
        #[serde(rename = "_rift", skip_serializing_if = "Option::is_none")]
        rift: Option<RiftResponseExtension>,
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
    /// Rift script-only response (no `is` block, response generated by script)
    RiftScript {
        rift: RiftResponseExtension,
    },
}

/// Raw deserialization type that handles multiple JSON formats for stub responses
/// Supports:
/// - Standard Mountebank format with `is`, `proxy`, `inject`, or `fault` fields
/// - Formats with `behaviors` (without underscore) or `_behaviors`
/// - Formats with `proxy: null` alongside `is` (ignored)
/// - `statusCode` as either string or number
/// - Rift extensions via `_rift` field
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StubResponseRaw {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is: Option<IsResponseRaw>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<String>,
    /// Mountebank-style behaviors (with underscore prefix) - for deserialization
    #[serde(rename = "_behaviors", skip_serializing_if = "Option::is_none")]
    pub underscore_behaviors: Option<serde_json::Value>,
    /// Alternative behaviors field (without underscore, used by some tools) - for deserialization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behaviors: Option<serde_json::Value>,
    /// Rift extensions for advanced features
    #[serde(rename = "_rift", skip_serializing_if = "Option::is_none")]
    pub rift: Option<RiftResponseExtension>,
}

/// Serialization type for stub responses - outputs Mountebank-compatible format
/// Uses `behaviors` as array (Mountebank standard format)
/// Field ordering matches Mountebank: behaviors, is, proxy
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StubResponseOut {
    /// Mountebank-style behaviors as array (standard Mountebank output format)
    /// Placed first to match Mountebank output ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behaviors: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is: Option<IsResponseOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<String>,
    /// Rift extensions for advanced features
    #[serde(rename = "_rift", skip_serializing_if = "Option::is_none")]
    pub rift: Option<RiftResponseExtension>,
}

/// Raw IsResponse that handles statusCode as string or number (for deserialization)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IsResponseRaw {
    #[serde(
        default = "default_status_code",
        deserialize_with = "deserialize_status_code"
    )]
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Response mode: "text" (default) or "binary" (body is base64-encoded)
    #[serde(rename = "_mode", default)]
    pub mode: ResponseMode,
}

/// IsResponse for serialization - outputs statusCode as string (Mountebank format)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IsResponseOut {
    /// Status code serialized as string for Mountebank compatibility
    #[serde(serialize_with = "serialize_status_code_as_string")]
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Response mode: "text" (default) or "binary" (body is base64-encoded)
    /// Skipped when text (default) as Mountebank doesn't output it for text mode
    #[serde(rename = "_mode", default, skip_serializing_if = "is_text_mode")]
    pub mode: ResponseMode,
}

/// Serialize statusCode as a string for Mountebank compatibility
fn serialize_status_code_as_string<S>(status_code: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&status_code.to_string())
}

pub(crate) fn default_status_code() -> u16 {
    200
}

/// Deserialize statusCode from either a number or a string
pub(crate) fn deserialize_status_code<'de, D>(deserializer: D) -> Result<u16, D::Error>
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
        // Priority: is > proxy > inject > fault > rift-script-only
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
                    mode: is_raw.mode,
                },
                behaviors,
                rift: raw.rift,
            }
        } else if let Some(proxy) = raw.proxy {
            StubResponse::Proxy { proxy }
        } else if let Some(inject) = raw.inject {
            StubResponse::Inject { inject }
        } else if let Some(fault) = raw.fault {
            StubResponse::Fault { fault }
        } else if let Some(rift) = raw.rift {
            // Rift-only response (script generates the response)
            StubResponse::RiftScript { rift }
        } else {
            // Default to empty Is response
            StubResponse::Is {
                is: IsResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: None,
                    mode: ResponseMode::Text,
                },
                behaviors: None,
                rift: None,
            }
        }
    }
}

impl From<StubResponse> for StubResponseOut {
    fn from(response: StubResponse) -> Self {
        match response {
            StubResponse::Is {
                is,
                behaviors,
                rift,
            } => StubResponseOut {
                is: Some(IsResponseOut {
                    status_code: is.status_code,
                    headers: is.headers,
                    body: is.body,
                    mode: is.mode,
                }),
                proxy: None,
                inject: None,
                fault: None,
                // Convert behaviors object to array format for Mountebank compatibility
                behaviors: behaviors.and_then(behaviors_to_array),
                rift,
            },
            StubResponse::Proxy { proxy } => StubResponseOut {
                is: None,
                proxy: Some(proxy),
                inject: None,
                fault: None,
                behaviors: None,
                rift: None,
            },
            StubResponse::Inject { inject } => StubResponseOut {
                is: None,
                proxy: None,
                inject: Some(inject),
                fault: None,
                behaviors: None,
                rift: None,
            },
            StubResponse::Fault { fault } => StubResponseOut {
                is: None,
                proxy: None,
                inject: None,
                fault: Some(fault),
                behaviors: None,
                rift: None,
            },
            StubResponse::RiftScript { rift } => StubResponseOut {
                is: None,
                proxy: None,
                inject: None,
                fault: None,
                behaviors: None,
                rift: Some(rift),
            },
        }
    }
}

/// Convert behaviors from object format to array format for Mountebank compatibility
/// Mountebank outputs: `"behaviors": [{"wait": ...}, {"decorate": ...}]`
/// Rift internally stores as object: `{"wait": ..., "decorate": ...}`
fn behaviors_to_array(value: serde_json::Value) -> Option<Vec<serde_json::Value>> {
    match value {
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                None
            } else {
                // Convert each key-value pair to a separate object in the array
                let arr: Vec<serde_json::Value> = obj
                    .into_iter()
                    .map(|(k, v)| {
                        let mut m = serde_json::Map::new();
                        m.insert(k, v);
                        serde_json::Value::Object(m)
                    })
                    .collect();
                Some(arr)
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                None
            } else {
                Some(arr)
            }
        }
        _ => None,
    }
}

/// Normalize behaviors from array format to object format
/// Some tools use `behaviors: [{"wait": ...}, {"decorate": ...}]` instead of
/// `_behaviors: {"wait": ..., "decorate": ...}`
pub(crate) fn normalize_behaviors(value: serde_json::Value) -> Option<serde_json::Value> {
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

/// Response mode for body handling (Mountebank compatible)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseMode {
    /// Body is UTF-8 text (default)
    #[default]
    Text,
    /// Body is base64-encoded binary data
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IsResponse {
    #[serde(default = "default_status_code")]
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Response mode: "text" (default) or "binary" (body is base64-encoded)
    #[serde(rename = "_mode", default, skip_serializing_if = "is_text_mode")]
    pub mode: ResponseMode,
}

fn is_text_mode(mode: &ResponseMode) -> bool {
    *mode == ResponseMode::Text
}

/// Path rewrite configuration for proxy responses (Mountebank compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRewrite {
    /// Pattern to match in the path (string to replace)
    pub from: String,
    /// Replacement string
    pub to: String,
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
    /// Path rewrite configuration for transforming the request path before proxying
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_rewrite: Option<PathRewrite>,
}

// ============================================================================
// Imposter Config
// ============================================================================

fn default_protocol() -> String {
    "http".to_string()
}

/// Configuration for creating an imposter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImposterConfig {
    /// Port for the imposter. If not specified, an available port will be auto-assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Host/IP address to bind the imposter to. Defaults to "0.0.0.0" (all interfaces).
    /// Use "127.0.0.1" or "localhost" for local-only access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub record_requests: bool,
    /// Record which stub matched each request (Mountebank compatible)
    #[serde(default)]
    pub record_matches: bool,
    #[serde(default)]
    pub stubs: Vec<Stub>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_response: Option<IsResponse>,
    /// Allow CORS headers (Mountebank compatible)
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        alias = "allowCORS"
    )]
    pub allow_cors: bool,
    /// Service name for documentation (optional metadata)
    #[serde(skip_serializing_if = "Option::is_none", alias = "service_name")]
    pub service_name: Option<String>,
    /// Service info for documentation (optional metadata, stored as-is)
    #[serde(skip_serializing_if = "Option::is_none", alias = "service_info")]
    pub service_info: Option<serde_json::Value>,
    /// Rift extensions for advanced features (flow state, scripting, faults)
    #[serde(rename = "_rift", default, skip_serializing_if = "Option::is_none")]
    pub rift: Option<RiftConfig>,
}

// ============================================================================
// Rift Extension Types (_rift namespace)
// ============================================================================

/// Top-level Rift configuration block for imposters
/// Extends Mountebank format with advanced features while maintaining backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RiftConfig {
    /// Flow state configuration (enables stateful scripting)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_state: Option<RiftFlowStateConfig>,
    /// Metrics configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RiftMetricsConfig>,
    /// Proxy/upstream configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<RiftProxyConfig>,
    /// Global script engine configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_engine: Option<RiftScriptEngineConfig>,
}

/// Flow state configuration for Rift extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftFlowStateConfig {
    /// Backend type: "inmemory" or "redis"
    #[serde(default = "default_flow_backend")]
    pub backend: String,
    /// Default TTL for state entries in seconds
    #[serde(default = "default_flow_ttl")]
    pub ttl_seconds: i64,
    /// Redis configuration (required when backend is "redis")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redis: Option<RiftRedisConfig>,
    /// Mountebank state mapping configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mountebank_state_mapping: Option<MountebankStateMapping>,
}

fn default_flow_backend() -> String {
    "inmemory".to_string()
}

fn default_flow_ttl() -> i64 {
    300
}

impl Default for RiftFlowStateConfig {
    fn default() -> Self {
        Self {
            backend: default_flow_backend(),
            ttl_seconds: default_flow_ttl(),
            redis: None,
            mountebank_state_mapping: None,
        }
    }
}

/// Redis configuration for flow state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftRedisConfig {
    /// Redis connection URL
    pub url: String,
    /// Connection pool size
    #[serde(default = "default_redis_pool")]
    pub pool_size: usize,
    /// Key prefix for all flow state keys
    #[serde(default = "default_redis_prefix")]
    pub key_prefix: String,
}

fn default_redis_pool() -> usize {
    10
}

fn default_redis_prefix() -> String {
    "rift:".to_string()
}

/// Configuration for bridging Mountebank state to flow store
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountebankStateMapping {
    /// Enable state mapping
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Source for flow_id: "imposter_port" or "header:X-Header-Name"
    #[serde(default = "default_flow_id_source")]
    pub flow_id_source: String,
}

fn default_true() -> bool {
    true
}

fn default_flow_id_source() -> String {
    "imposter_port".to_string()
}

/// Metrics configuration for Rift extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftMetricsConfig {
    /// Enable metrics collection
    #[serde(default)]
    pub enabled: bool,
    /// Metrics server port
    #[serde(default = "default_metrics_port")]
    pub port: u16,
}

fn default_metrics_port() -> u16 {
    9090
}

/// Proxy configuration for Rift extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftProxyConfig {
    /// Upstream target configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<RiftUpstreamConfig>,
    /// Connection pool settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_pool: Option<RiftConnectionPoolConfig>,
}

/// Upstream configuration for Rift proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftUpstreamConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_upstream_protocol")]
    pub protocol: String,
}

fn default_upstream_protocol() -> String {
    "http".to_string()
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftConnectionPoolConfig {
    #[serde(default = "default_max_idle")]
    pub max_idle_per_host: usize,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_max_idle() -> usize {
    100
}

fn default_idle_timeout() -> u64 {
    90
}

/// Global script engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftScriptEngineConfig {
    /// Default script engine: "rhai", "lua", or "javascript"
    #[serde(default = "default_script_engine")]
    pub default_engine: String,
    /// Script execution timeout in milliseconds
    #[serde(default = "default_script_timeout")]
    pub timeout_ms: u64,
}

fn default_script_engine() -> String {
    "rhai".to_string()
}

fn default_script_timeout() -> u64 {
    5000
}

/// Rift response extensions (added to stub responses)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RiftResponseExtension {
    /// Fault injection configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<RiftFaultConfig>,
    /// Script-based response generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<RiftScriptConfig>,
}

/// Fault injection configuration for responses
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RiftFaultConfig {
    /// Latency injection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<RiftLatencyFault>,
    /// Error injection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RiftErrorFault>,
    /// TCP-level fault
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp: Option<String>,
}

/// Latency fault configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftLatencyFault {
    /// Probability of fault injection (0.0 to 1.0)
    #[serde(default = "default_probability")]
    pub probability: f64,
    /// Minimum latency in milliseconds
    #[serde(default)]
    pub min_ms: u64,
    /// Maximum latency in milliseconds
    #[serde(default)]
    pub max_ms: u64,
    /// Fixed latency (alternative to min/max)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
}

fn default_probability() -> f64 {
    1.0
}

/// Error fault configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftErrorFault {
    /// Probability of error injection (0.0 to 1.0)
    #[serde(default = "default_probability")]
    pub probability: f64,
    /// HTTP status code for error response
    #[serde(default = "default_error_status")]
    pub status: u16,
    /// Response body for error
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Custom headers for error response
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

fn default_error_status() -> u16 {
    503
}

/// Script configuration for response generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiftScriptConfig {
    /// Script engine: "rhai", "lua", or "javascript"
    #[serde(default = "default_script_engine")]
    pub engine: String,
    /// Inline script code
    pub code: String,
}

// ============================================================================
// Error Types
// ============================================================================

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

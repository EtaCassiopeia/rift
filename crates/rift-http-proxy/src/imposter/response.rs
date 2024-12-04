//! Response building and execution logic for imposters.
//!
//! This module handles creating responses from stubs, applying behaviors,
//! and managing the response cycle.

use crate::behaviors::{apply_decorate, HasRepeatBehavior, RequestContext};
use std::collections::HashMap;

use super::types::{
    DebugResponsePreview, IsResponse, ResponseMode, RiftResponseExtension, RiftScriptConfig,
    StubResponse,
};

// Implement HasRepeatBehavior for StubResponse
impl HasRepeatBehavior for StubResponse {
    fn get_repeat(&self) -> Option<u32> {
        match self {
            StubResponse::Is { behaviors, .. } => behaviors
                .as_ref()
                .and_then(|b| b.get("repeat"))
                .and_then(|r| r.as_u64())
                .map(|r| r as u32),
            StubResponse::RiftScript { .. } => None,
            _ => None,
        }
    }
}

/// Create response preview from a StubResponse (for debug mode)
pub fn create_response_preview(response: &StubResponse) -> DebugResponsePreview {
    match response {
        StubResponse::Is { is, .. } => {
            let body_preview = is.body.as_ref().map(|b| match b {
                serde_json::Value::String(s) => {
                    if s.len() > 500 {
                        format!("{}...", &s[..500])
                    } else {
                        s.clone()
                    }
                }
                other => {
                    let json = serde_json::to_string(other).unwrap_or_default();
                    if json.len() > 500 {
                        format!("{}...", &json[..500])
                    } else {
                        json
                    }
                }
            });
            let headers = if is.headers.is_empty() {
                None
            } else {
                Some(
                    is.headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            };
            DebugResponsePreview {
                response_type: "is".to_string(),
                status_code: Some(is.status_code),
                headers,
                body_preview,
            }
        }
        StubResponse::Proxy { proxy, .. } => DebugResponsePreview {
            response_type: "proxy".to_string(),
            status_code: None,
            headers: None,
            body_preview: Some(format!("Proxy to: {}", proxy.to)),
        },
        StubResponse::Inject { inject, .. } => DebugResponsePreview {
            response_type: "inject".to_string(),
            status_code: None,
            headers: None,
            body_preview: Some(format!(
                "JavaScript inject: {}...",
                if inject.len() > 50 {
                    &inject[..50]
                } else {
                    inject
                }
            )),
        },
        StubResponse::Fault { fault, .. } => DebugResponsePreview {
            response_type: "fault".to_string(),
            status_code: None,
            headers: None,
            body_preview: Some(format!("Fault: {fault}")),
        },
        StubResponse::RiftScript { rift } => {
            // RiftScript uses the _rift extension namespace
            let script_info = if rift.script.is_some() {
                "Rift script response"
            } else if rift.fault.is_some() {
                "Rift fault injection"
            } else {
                "Rift extension response"
            };
            DebugResponsePreview {
                response_type: "_rift".to_string(),
                status_code: None,
                headers: None,
                body_preview: Some(script_info.to_string()),
            }
        }
    }
}

/// Execute a stub response and return (status, headers, body, behaviors, is_fault)
#[allow(clippy::type_complexity)]
pub fn execute_stub_response(
    response: &StubResponse,
) -> Option<(
    u16,
    HashMap<String, String>,
    String,
    Option<serde_json::Value>,
    bool,
)> {
    match response {
        StubResponse::Is { is, behaviors, .. } => {
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
                            headers
                                .insert("Content-Type".to_string(), "application/json".to_string());
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
        StubResponse::Inject { .. } => None, // Inject handled via get_inject_response
        StubResponse::RiftScript { .. } => None, // Handled via get_rift_script_response
    }
}

/// Execute a stub response with Rift extensions
/// Returns (status, headers, body, behaviors, rift_extension, response_mode, is_fault)
#[allow(clippy::type_complexity)]
pub fn execute_stub_response_with_rift(
    response: &StubResponse,
) -> Option<(
    u16,
    HashMap<String, String>,
    String,
    Option<serde_json::Value>,
    Option<RiftResponseExtension>,
    ResponseMode,
    bool,
)> {
    match response {
        StubResponse::Is {
            is,
            behaviors,
            rift,
        } => {
            let mut headers = is.headers.clone();
            let mode = is.mode.clone();

            let body = is
                .body
                .as_ref()
                .map(|b| {
                    if b.is_string() {
                        b.as_str().unwrap_or("").to_string()
                    } else {
                        if !headers.contains_key("content-type")
                            && !headers.contains_key("Content-Type")
                        {
                            headers
                                .insert("Content-Type".to_string(), "application/json".to_string());
                        }
                        serde_json::to_string(b).unwrap_or_default()
                    }
                })
                .unwrap_or_default();

            Some((
                is.status_code,
                headers,
                body,
                behaviors.clone(),
                rift.clone(),
                mode,
                false,
            ))
        }
        StubResponse::Fault { fault } => Some((
            0,
            HashMap::new(),
            fault.clone(),
            None,
            None,
            ResponseMode::Text,
            true,
        )),
        StubResponse::Proxy { .. } => None,
        StubResponse::Inject { .. } => None,
        StubResponse::RiftScript { .. } => None,
    }
}

/// Get RiftScript config if the response is a RiftScript type
pub fn get_rift_script_config(response: &StubResponse) -> Option<RiftScriptConfig> {
    match response {
        StubResponse::RiftScript { rift } => rift.script.clone(),
        _ => None,
    }
}

/// Create a stub from a recorded proxy response
pub fn create_stub_from_proxy_response(
    predicates: Vec<serde_json::Value>,
    status: u16,
    headers: &HashMap<String, String>,
    body: &str,
    latency_ms: Option<u64>,
    decorate_fn: Option<String>,
) -> super::types::Stub {
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
        mode: ResponseMode::Text, // Proxy responses are always text
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

    super::types::Stub {
        id: None,
        predicates,
        responses: vec![StubResponse::Is {
            is: is_response,
            behaviors,
            rift: None,
        }],
        scenario_name: None,
    }
}

/// Apply decorate behavior - handles both JavaScript and Rhai scripts
pub fn apply_js_or_rhai_decorate(
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

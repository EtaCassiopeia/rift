//! Request handling logic for imposters.
//!
//! This module handles incoming HTTP requests to imposters, including
//! debug mode, proxy handling, inject execution, and response generation.

use super::core::Imposter;
use super::predicates::parse_query_string;
use super::response::apply_js_or_rhai_decorate;
use super::types::{DebugMatchResult, DebugRequest, DebugResponse, RecordedRequest, ResponseMode};
use crate::admin_api::types::{build_response, build_response_with_headers};
use crate::behaviors::{
    apply_copy_behaviors, header_to_title_case, RequestContext, ResponseBehaviors,
};
#[cfg(feature = "javascript")]
use crate::scripting::{execute_mountebank_inject, MountebankRequest};
use crate::scripting::{FaultDecision, ScriptEngine, ScriptRequest};
use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use rand::Rng;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Handle a request to an imposter
pub async fn handle_imposter_request(
    req: Request<Incoming>,
    imposter: Arc<Imposter>,
    client_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Check if enabled
    if !imposter.is_enabled() {
        return Ok(build_response_with_headers(
            StatusCode::SERVICE_UNAVAILABLE,
            [("x-rift-imposter-disabled", "true")],
            r#"{"error": "Imposter is disabled"}"#,
        ));
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
            (
                header_to_title_case(k.as_str()),
                v.to_str().unwrap_or("").to_string(),
            )
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

    // Check for X-Rift-Debug header (Rift extension)
    // If present, return match information instead of processing the request
    let is_debug_mode = headers_clone
        .get("X-Rift-Debug")
        .or_else(|| headers_clone.get("x-rift-debug"))
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);

    if is_debug_mode {
        return handle_debug_request(
            &imposter,
            &method,
            &path,
            &query_str,
            &headers_clone,
            &body_string,
            &headers_for_context,
            client_addr,
        );
    }

    // Get client address info for requestFrom, ip predicates
    let request_from = client_addr.to_string();
    let client_ip = client_addr.ip().to_string();

    if let Some((stub, stub_index)) = imposter.find_matching_stub_with_client(
        method_str,
        path_str,
        &headers_for_context,
        query_opt,
        body_string.as_deref(),
        Some(&request_from),
        Some(&client_ip),
    ) {
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

                    return Ok(response
                        .body(Full::new(Bytes::from(body)))
                        .unwrap_or_else(|_| {
                            build_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Response build error",
                            )
                        }));
                }
                Err(e) => {
                    warn!("Proxy request failed: {}", e);
                    return Ok(build_response_with_headers(
                        StatusCode::BAD_GATEWAY,
                        [("x-rift-imposter", "true"), ("x-rift-proxy-error", "true")],
                        format!(r#"{{"error": "Proxy error: {e}"}}"#),
                    ));
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

            match execute_mountebank_inject(
                &inject_fn,
                &mb_request,
                imposter.config.port.unwrap_or(0),
            ) {
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
                        .unwrap_or_else(|_| {
                            build_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Response build error",
                            )
                        }));
                }
                Err(e) => {
                    warn!("Inject function failed: {}", e);
                    return Ok(build_response_with_headers(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [("x-rift-imposter", "true"), ("x-rift-inject-error", "true")],
                        format!(r#"{{"error": "Inject error: {e}"}}"#),
                    ));
                }
            }
        }

        // Check if this is a RiftScript response (_rift.script)
        if let Some(script_config) = imposter.get_rift_script_response(&stub, stub_index) {
            debug!(
                "Handling Rift script response (engine: {})",
                script_config.engine
            );

            // Build script request
            let script_request = ScriptRequest {
                method: method.clone(),
                path: path.clone(),
                headers: headers_clone.clone(),
                body: body_string
                    .as_ref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Null),
                query: parse_query_string(&query_str),
                path_params: HashMap::new(),
            };

            // Create script engine and execute
            match ScriptEngine::new(
                &script_config.engine,
                &script_config.code,
                format!("rift_script_{stub_index}"),
            ) {
                Ok(engine) => {
                    let flow_store = imposter.flow_store.clone();
                    match engine.should_inject_fault(&script_request, flow_store) {
                        Ok(FaultDecision::Error {
                            status,
                            body,
                            headers,
                            ..
                        }) => {
                            imposter.advance_cycler_for_rift_script(&stub, stub_index);

                            let mut response = Response::builder().status(status);
                            for (k, v) in &headers {
                                response = response.header(k, v);
                            }
                            response = response.header("x-rift-imposter", "true");
                            response = response.header("x-rift-script", &script_config.engine);

                            return Ok(response.body(Full::new(Bytes::from(body))).unwrap_or_else(
                                |_| {
                                    build_response(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Response build error",
                                    )
                                },
                            ));
                        }
                        Ok(FaultDecision::Latency { duration_ms, .. }) => {
                            // Apply latency then return 200 OK
                            tokio::time::sleep(Duration::from_millis(duration_ms)).await;
                            imposter.advance_cycler_for_rift_script(&stub, stub_index);

                            return Ok(build_response_with_headers(
                                StatusCode::OK,
                                [
                                    ("x-rift-imposter", "true"),
                                    ("x-rift-script", &script_config.engine),
                                    ("x-rift-latency-ms", &duration_ms.to_string()),
                                ],
                                Bytes::new(),
                            ));
                        }
                        Ok(FaultDecision::None) => {
                            // Script says no fault - return 200 OK
                            imposter.advance_cycler_for_rift_script(&stub, stub_index);

                            return Ok(build_response_with_headers(
                                StatusCode::OK,
                                [
                                    ("x-rift-imposter", "true"),
                                    ("x-rift-script", script_config.engine.as_str()),
                                ],
                                Bytes::new(),
                            ));
                        }
                        Err(e) => {
                            warn!("Rift script execution failed: {}", e);
                            return Ok(build_response_with_headers(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                [("x-rift-imposter", "true"), ("x-rift-script-error", "true")],
                                format!(r#"{{"error": "Script error: {e}"}}"#),
                            ));
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to create script engine: {}", e);
                    return Ok(build_response_with_headers(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [("x-rift-imposter", "true"), ("x-rift-script-error", "true")],
                        format!(r#"{{"error": "Script engine error: {e}"}}"#),
                    ));
                }
            }
        }

        if let Some((
            mut status,
            mut headers,
            mut body,
            behaviors,
            rift_ext,
            response_mode,
            is_fault,
        )) = imposter.execute_stub_with_rift(&stub, stub_index)
        {
            // Handle faults - simulate connection errors
            if is_fault {
                return handle_fault_response(&body);
            }

            // Apply _rift.fault extensions (probabilistic faults)
            if let Some(ref rift) = rift_ext {
                if let Some(ref fault_config) = rift.fault {
                    if let Some(response) =
                        apply_rift_fault(fault_config, &mut status, &mut body).await
                    {
                        return Ok(response);
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

            // Handle binary mode - decode base64 body if _mode is "binary"
            let body_bytes = match response_mode {
                ResponseMode::Binary => {
                    // Decode base64-encoded body
                    match base64::engine::general_purpose::STANDARD.decode(&body) {
                        Ok(decoded) => Bytes::from(decoded),
                        Err(e) => {
                            warn!("Failed to decode base64 body: {}, using raw body", e);
                            Bytes::from(body)
                        }
                    }
                }
                ResponseMode::Text => Bytes::from(body),
            };

            return Ok(response.body(Full::new(body_bytes)).unwrap_or_else(|_| {
                build_response(StatusCode::INTERNAL_SERVER_ERROR, "Response build error")
            }));
        }
    }

    // No matching rule - return default response or 404
    if let Some(ref default) = imposter.config.default_response {
        let body_str = default
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

        // Handle binary mode for default response
        let body_bytes = match default.mode {
            ResponseMode::Binary => {
                match base64::engine::general_purpose::STANDARD.decode(&body_str) {
                    Ok(decoded) => Bytes::from(decoded),
                    Err(e) => {
                        warn!(
                            "Failed to decode base64 default body: {}, using raw body",
                            e
                        );
                        Bytes::from(body_str)
                    }
                }
            }
            ResponseMode::Text => Bytes::from(body_str),
        };

        let mut response = Response::builder().status(default.status_code);
        for (k, v) in &default.headers {
            response = response.header(k, v);
        }
        response = response.header("x-rift-imposter", "true");
        response = response.header("x-rift-default-response", "true");

        return Ok(response.body(Full::new(body_bytes)).unwrap_or_else(|_| {
            build_response(StatusCode::INTERNAL_SERVER_ERROR, "Response build error")
        }));
    }

    // No match and no default - Mountebank returns 200 with empty body
    Ok(build_response_with_headers(
        StatusCode::OK,
        [("x-rift-imposter", "true"), ("x-rift-no-match", "true")],
        Bytes::new(),
    ))
}

/// Handle debug mode request
#[allow(clippy::too_many_arguments)]
fn handle_debug_request(
    imposter: &Arc<Imposter>,
    method: &str,
    path: &str,
    query_str: &str,
    headers_clone: &HashMap<String, String>,
    body_string: &Option<String>,
    headers_for_context: &hyper::HeaderMap,
    client_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("Debug mode enabled for request {} {}", method, path);

    // Build debug request info
    let debug_request = DebugRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: if query_str.is_empty() {
            None
        } else {
            Some(query_str.to_string())
        },
        headers: headers_clone
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("x-rift-debug"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        body: body_string.clone(),
    };

    // Get imposter info
    let debug_imposter = imposter.get_debug_imposter_info();

    // Find matching stub for debug info (with client address)
    let request_from = client_addr.to_string();
    let client_ip = client_addr.ip().to_string();
    let query_opt = if query_str.is_empty() {
        None
    } else {
        Some(query_str)
    };

    let match_result = if let Some((stub, stub_index)) = imposter.find_matching_stub_with_client(
        method,
        path,
        headers_for_context,
        query_opt,
        body_string.as_deref(),
        Some(&request_from),
        Some(&client_ip),
    ) {
        // Match found
        let response_preview = imposter.get_response_preview(&stub, stub_index);
        DebugMatchResult {
            matched: true,
            stub_index: Some(stub_index),
            stub_id: stub.id.clone(),
            predicates: Some(stub.predicates.clone()),
            response_preview: Some(response_preview),
            all_stubs: None,
            reason: None,
        }
    } else {
        // No match - return all stubs for inspection
        let all_stubs = imposter.get_all_stubs_info();
        let reason = if all_stubs.is_empty() {
            "No stubs configured for this imposter".to_string()
        } else {
            "No stub predicates matched the request".to_string()
        };
        DebugMatchResult {
            matched: false,
            stub_index: None,
            stub_id: None,
            predicates: None,
            response_preview: None,
            all_stubs: Some(all_stubs),
            reason: Some(reason),
        }
    };

    let debug_response = DebugResponse {
        debug: true,
        request: debug_request,
        imposter: debug_imposter,
        match_result,
    };

    let json_body = serde_json::to_string_pretty(&debug_response)
        .unwrap_or_else(|_| r#"{"error": "Failed to serialize debug response"}"#.to_string());

    Ok(build_response_with_headers(
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            ("X-Rift-Debug-Response", "true"),
        ],
        json_body,
    ))
}

/// Handle fault response types
fn handle_fault_response(fault_type: &str) -> Result<Response<Full<Bytes>>, Infallible> {
    match fault_type {
        "CONNECTION_RESET_BY_PEER" => {
            // Return empty response to simulate connection reset
            // In real Mountebank, this would actually reset the TCP connection
            Ok(build_response_with_headers(
                StatusCode::BAD_GATEWAY,
                [("x-rift-fault", "CONNECTION_RESET_BY_PEER")],
                Bytes::new(),
            ))
        }
        "RANDOM_DATA_THEN_CLOSE" => Ok(build_response_with_headers(
            StatusCode::BAD_GATEWAY,
            [("x-rift-fault", "RANDOM_DATA_THEN_CLOSE")],
            Bytes::from_static(b"\x00\xff\xfe\xfd"),
        )),
        _ => Ok(build_response_with_headers(
            StatusCode::INTERNAL_SERVER_ERROR,
            [("x-rift-fault", fault_type)],
            format!("Unknown fault: {fault_type}"),
        )),
    }
}

/// Apply Rift fault configuration (probabilistic faults)
async fn apply_rift_fault(
    fault_config: &super::types::RiftFaultConfig,
    _status: &mut u16,
    _body: &mut String,
) -> Option<Response<Full<Bytes>>> {
    // Generate all random values before any await points (ThreadRng is not Send)
    let (apply_latency, latency_delay_ms) = {
        let mut rng = rand::thread_rng();
        if let Some(ref latency) = fault_config.latency {
            if rng.gen::<f64>() < latency.probability {
                let delay_ms = if let Some(fixed_ms) = latency.ms {
                    fixed_ms
                } else if latency.max_ms > latency.min_ms {
                    rng.gen_range(latency.min_ms..=latency.max_ms)
                } else {
                    latency.min_ms
                };
                (true, delay_ms)
            } else {
                (false, 0)
            }
        } else {
            (false, 0)
        }
    };

    let apply_error = {
        let mut rng = rand::thread_rng();
        if let Some(ref error) = fault_config.error {
            rng.gen::<f64>() < error.probability
        } else {
            false
        }
    };

    // Apply latency fault (this is async)
    if apply_latency && latency_delay_ms > 0 {
        debug!("Applying _rift.fault latency: {}ms", latency_delay_ms);
        tokio::time::sleep(Duration::from_millis(latency_delay_ms)).await;
    }

    // Apply error fault
    if apply_error {
        if let Some(ref error) = fault_config.error {
            debug!("Applying _rift.fault error: status {}", error.status);

            let mut response = Response::builder().status(error.status);

            // Apply custom headers
            for (k, v) in &error.headers {
                response = response.header(k, v);
            }

            response = response.header("x-rift-imposter", "true");
            response = response.header("x-rift-fault", "error");

            let error_body = error.body.clone().unwrap_or_default();
            return Some(
                response
                    .body(Full::new(Bytes::from(error_body)))
                    .unwrap_or_else(|_| {
                        build_response(StatusCode::INTERNAL_SERVER_ERROR, "Response build error")
                    }),
            );
        }
    }

    // Check for TCP fault
    if let Some(ref tcp_fault) = fault_config.tcp {
        match tcp_fault.as_str() {
            "reset" | "CONNECTION_RESET_BY_PEER" => {
                debug!("Applying _rift.fault TCP reset");
                return Some(build_response_with_headers(
                    StatusCode::BAD_GATEWAY,
                    [("x-rift-fault", "CONNECTION_RESET_BY_PEER")],
                    Bytes::new(),
                ));
            }
            "garbage" | "RANDOM_DATA_THEN_CLOSE" => {
                debug!("Applying _rift.fault TCP garbage");
                return Some(build_response_with_headers(
                    StatusCode::BAD_GATEWAY,
                    [("x-rift-fault", "RANDOM_DATA_THEN_CLOSE")],
                    Bytes::from_static(b"\x00\xff\xfe\xfd"),
                ));
            }
            _ => {
                warn!("Unknown TCP fault type: {}", tcp_fault);
            }
        }
    }

    None
}

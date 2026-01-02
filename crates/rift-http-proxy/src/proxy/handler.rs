//! Request handling and fault injection logic.
//!
//! This module contains the core request handling logic including:
//! - Script rule matching and execution
//! - YAML rule matching and fault injection
//! - Response behavior application (wait, copy, lookup, shell, decorate)

use super::client::HttpClient;
use super::forwarding::{error_response, forward_request_with_body, forward_with_recording};
use super::headers::{
    RiftHeadersExt, VALUE_ERROR, VALUE_LATENCY, VALUE_TCP, VALUE_TRUE, X_RIFT_BEHAVIOR_COPY,
    X_RIFT_BEHAVIOR_DECORATE, X_RIFT_BEHAVIOR_LOOKUP, X_RIFT_BEHAVIOR_SHELL, X_RIFT_BEHAVIOR_WAIT,
    X_RIFT_FAULT, X_RIFT_LATENCY_MS, X_RIFT_RULE_ID, X_RIFT_SCRIPT, X_RIFT_TCP_FAULT,
};
use super::response_ext::ResponseExt;
use crate::behaviors::{
    apply_copy_behaviors, apply_decorate, apply_lookup_behaviors, apply_shell_transform, CsvCache,
    RequestContext,
};
use crate::config::TcpFault;
use crate::extensions::fault::{apply_latency, create_error_response, decide_fault, FaultDecision};
use crate::extensions::flow_state::FlowStore;
use crate::extensions::matcher::CompiledRule;
use crate::extensions::metrics;
use crate::extensions::routing::Router;
use crate::extensions::template::{has_template_variables, process_template, RequestData};
use crate::recording::RecordingStore;
use crate::scripting::{
    CacheKey, CompiledScript, DecisionCache, FaultDecision as ScriptFaultDecision, ScriptPool,
    ScriptRequest,
};
use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper::{Request, Response};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Context for handling a request, containing all necessary state.
pub struct RequestHandlerContext<'a> {
    pub http_client: &'a HttpClient,
    pub compiled_rules: &'a [CompiledRule],
    pub rule_upstreams: &'a [Option<String>],
    pub upstream_uri: &'a str,
    pub router: Option<&'a Router>,
    pub upstreams: &'a [crate::config::Upstream],
    pub flow_store: &'a Arc<dyn FlowStore>,
    pub script_pool: Option<&'a Arc<ScriptPool>>,
    pub compiled_scripts: Option<&'a [(CompiledScript, CompiledRule, Option<String>)]>,
    pub decision_cache: Option<&'a Arc<DecisionCache>>,
    pub csv_cache: &'a Arc<CsvCache>,
    pub recording_store: &'a Arc<RecordingStore>,
    pub recording_signature_headers: &'a [(String, String)],
    pub flow_state_configured: bool,
}

/// Handle an incoming request with fault injection and forwarding.
pub async fn handle_request(
    ctx: &RequestHandlerContext<'_>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let start_time = std::time::Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    debug!("Received request: {} {}", method, uri);

    // Select upstream for this request (reverse proxy mode)
    let selected_upstream = select_upstream(ctx.router, ctx.upstreams, &req);
    let (selected_upstream_url, selected_upstream_name) = match selected_upstream {
        Some((url, name)) => (Some(url), Some(name)),
        None => (None, None),
    };

    // Check script rules first (if configured) - optimized path with pool and cache
    let req = if let (Some(compiled_scripts), Some(script_pool), Some(decision_cache)) =
        (ctx.compiled_scripts, ctx.script_pool, ctx.decision_cache)
    {
        match handle_script_rules(
            ctx,
            compiled_scripts,
            script_pool,
            decision_cache,
            req,
            &method,
            &uri,
            &headers,
            selected_upstream_url.as_deref(),
            selected_upstream_name.as_deref(),
            start_time,
        )
        .await
        {
            RuleHandlingResult::Response(response) => return Ok(response),
            RuleHandlingResult::NoFault(req) => req,
        }
    } else {
        req
    };

    // Find matching YAML rule that applies to selected upstream
    let matched_rule_index = ctx
        .compiled_rules
        .iter()
        .enumerate()
        .find(|(idx, rule)| {
            rule.matches(&method, &uri, &headers)
                && rule_applies_to_upstream(
                    &ctx.rule_upstreams[*idx],
                    selected_upstream_name.as_deref(),
                )
        })
        .map(|(idx, _)| idx);

    if let Some(rule_idx) = matched_rule_index {
        let rule = &ctx.compiled_rules[rule_idx];
        info!("Request matched rule: {}", rule.id);

        match handle_yaml_rule(
            ctx,
            rule,
            req,
            &method,
            &uri,
            &headers,
            selected_upstream_url.as_deref(),
            start_time,
        )
        .await
        {
            RuleHandlingResult::Response(response) => return Ok(response),
            RuleHandlingResult::NoFault(r) => {
                // Continue to forward without fault
                let upstream_url = selected_upstream_url.as_deref().unwrap_or(ctx.upstream_uri);
                let response = forward_with_recording(
                    ctx.http_client,
                    ctx.recording_store,
                    ctx.recording_signature_headers,
                    r,
                    upstream_url,
                )
                .await;
                let status = response.status().as_u16();
                let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
                metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
                metrics::record_request(method.as_str(), status);
                return Ok(response);
            }
        }
    }

    // Forward request without fault (with recording support if enabled)
    let upstream_url = selected_upstream_url.as_deref().unwrap_or(ctx.upstream_uri);
    let response = forward_with_recording(
        ctx.http_client,
        ctx.recording_store,
        ctx.recording_signature_headers,
        req,
        upstream_url,
    )
    .await;
    let status = response.status().as_u16();
    let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
    metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
    metrics::record_request(method.as_str(), status);
    Ok(response)
}

/// Result of rule handling - either a response or the request back
pub enum RuleHandlingResult {
    /// A rule matched and returned a response
    Response(Response<BoxBody<Bytes, hyper::Error>>),
    /// No fault injected, here's the request back for forwarding
    NoFault(Request<hyper::body::Incoming>),
}

/// Handle script rules - returns either a response or the request back if no script matched.
#[allow(clippy::too_many_arguments)]
async fn handle_script_rules(
    ctx: &RequestHandlerContext<'_>,
    compiled_scripts: &[(CompiledScript, CompiledRule, Option<String>)],
    script_pool: &Arc<ScriptPool>,
    decision_cache: &Arc<DecisionCache>,
    req: Request<hyper::body::Incoming>,
    method: &hyper::Method,
    uri: &hyper::Uri,
    headers: &hyper::HeaderMap,
    selected_upstream_url: Option<&str>,
    selected_upstream_name: Option<&str>,
    start_time: std::time::Instant,
) -> RuleHandlingResult {
    // Find first matching script rule that applies to selected upstream
    let matching_script = compiled_scripts
        .iter()
        .find(|(_, compiled_rule, rule_upstream)| {
            compiled_rule.matches(method, uri, headers)
                && rule_applies_to_upstream(rule_upstream, selected_upstream_name)
        });

    let (compiled_script, compiled_rule, _) = match matching_script {
        Some(m) => m,
        None => return RuleHandlingResult::NoFault(req),
    };
    info!("Request matched script rule: {}", compiled_rule.id);

    // Collect body for script (needed for script context)
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to collect request body: {}", e);
            return RuleHandlingResult::Response(
                error_response(500, "Failed to read request body").into_boxed(),
            );
        }
    };

    // Convert to script request
    let mut headers_map = HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(value_str) = v.to_str() {
            headers_map.insert(k.as_str().to_string(), value_str.to_string());
        }
    }

    let body_json: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

    // Parse query parameters from URI
    let query_params = crate::predicate::parse_query_string(uri.query());

    let script_request = ScriptRequest {
        method: method.to_string(),
        path: uri.path().to_string(),
        headers: headers_map.clone(),
        body: body_json.clone(),
        query: query_params,
        path_params: HashMap::new(),
    };

    // Create cache key
    let cache_key = CacheKey::new(
        method.to_string(),
        uri.path().to_string(),
        headers_map.into_iter().collect(),
        &body_json,
        compiled_rule.id.clone(),
    );

    // Determine if caching should be used
    // If flow_state is configured (not NoOpFlowStore), disable caching
    // because scripts using flow_store are stateful and results vary
    let use_cache = !ctx.flow_state_configured;

    // Check cache first (only for stateless scripts), then execute via pool
    let script_start = std::time::Instant::now();
    let result = if use_cache {
        if let Some(cached_decision) = decision_cache.get(&cache_key) {
            debug!("Cache hit for rule: {} (stateless)", compiled_rule.id);
            Ok(cached_decision)
        } else {
            debug!("Cache miss for rule: {}", compiled_rule.id);

            // Execute via pool
            let pool_result = script_pool
                .execute(
                    compiled_script.clone(),
                    script_request,
                    Arc::clone(ctx.flow_store),
                )
                .await;

            // Cache the result if successful (stateless only)
            if let Ok(ref decision) = pool_result {
                let _ = decision_cache.insert(cache_key, decision.clone());
            }

            pool_result
        }
    } else {
        // Stateful script: always execute, never cache
        debug!("Executing stateful script (no cache): {}", compiled_rule.id);
        script_pool
            .execute(
                compiled_script.clone(),
                script_request,
                Arc::clone(ctx.flow_store),
            )
            .await
    };
    let script_duration = script_start.elapsed().as_secs_f64() * 1000.0;

    RuleHandlingResult::Response(
        handle_script_result(
            ctx,
            result.map_err(|e| e.to_string()),
            compiled_rule,
            method,
            uri,
            headers,
            body_bytes,
            selected_upstream_url,
            selected_upstream_name,
            start_time,
            script_duration,
        )
        .await,
    )
}

/// Handle the result of a script execution.
#[allow(clippy::too_many_arguments)]
async fn handle_script_result(
    ctx: &RequestHandlerContext<'_>,
    result: Result<ScriptFaultDecision, String>,
    compiled_rule: &CompiledRule,
    method: &hyper::Method,
    uri: &hyper::Uri,
    headers: &hyper::HeaderMap,
    body_bytes: Bytes,
    selected_upstream_url: Option<&str>,
    selected_upstream_name: Option<&str>,
    start_time: std::time::Instant,
    script_duration: f64,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    match result {
        Ok(ScriptFaultDecision::Error {
            status,
            body,
            rule_id,
            headers: script_headers,
        }) => {
            warn!(
                "Script injecting error fault: status={}, rule={}",
                status, rule_id
            );

            // Record metrics
            metrics::record_script_execution(&rule_id, script_duration, "inject");
            metrics::record_script_fault("error", &rule_id, None);
            metrics::record_error_injection(&rule_id, status);

            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), duration_ms, "script");
            metrics::record_request(method.as_str(), status);

            // Find fixed headers from matching YAML rule (if any)
            let fixed_headers = ctx
                .compiled_rules
                .iter()
                .enumerate()
                .find(|(idx, rule)| {
                    rule.matches(method, uri, headers)
                        && rule_applies_to_upstream(
                            &ctx.rule_upstreams[*idx],
                            selected_upstream_name,
                        )
                        && rule.rule.fault.error.is_some()
                })
                .and_then(|(_, rule)| rule.rule.fault.error.as_ref().map(|e| e.headers.clone()));

            let mut response =
                create_error_response(status, body, fixed_headers.as_ref(), Some(&script_headers))
                    .unwrap();
            response.set_header(&X_RIFT_FAULT, &VALUE_ERROR);
            response.set_header_value(&X_RIFT_RULE_ID, &rule_id);
            response.set_header(&X_RIFT_SCRIPT, &VALUE_TRUE);
            response.into_boxed()
        }
        Ok(ScriptFaultDecision::Latency {
            duration_ms,
            rule_id,
        }) => {
            info!(
                "Script injecting latency fault: {}ms, rule={}",
                duration_ms, rule_id
            );

            // Record metrics
            metrics::record_script_execution(&rule_id, script_duration, "inject");
            metrics::record_script_fault("latency", &rule_id, Some(duration_ms));

            apply_latency(duration_ms).await;

            // Forward with body for latency fault
            let upstream_url = selected_upstream_url.unwrap_or(ctx.upstream_uri);
            let mut response = forward_request_with_body(
                ctx.http_client,
                method.clone(),
                uri.clone(),
                headers.clone(),
                body_bytes,
                upstream_url,
            )
            .await;
            let status = response.status().as_u16();

            let total_duration = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), total_duration, "script");
            metrics::record_request(method.as_str(), status);

            response.set_header(&X_RIFT_FAULT, &VALUE_LATENCY);
            response.set_header_value(&X_RIFT_RULE_ID, &rule_id);
            response.set_header(&X_RIFT_SCRIPT, &VALUE_TRUE);
            response.set_header_value(&X_RIFT_LATENCY_MS, &duration_ms.to_string());
            response.into_boxed()
        }
        Ok(ScriptFaultDecision::None) => {
            debug!(
                "Script decided not to inject fault for rule: {}",
                compiled_rule.id
            );
            metrics::record_script_execution(&compiled_rule.id, script_duration, "pass");

            // Forward request
            let upstream_url = selected_upstream_url.unwrap_or(ctx.upstream_uri);
            let response = forward_request_with_body(
                ctx.http_client,
                method.clone(),
                uri.clone(),
                headers.clone(),
                body_bytes,
                upstream_url,
            )
            .await;
            let status = response.status().as_u16();
            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
            metrics::record_request(method.as_str(), status);
            response.into_boxed()
        }
        Err(e) => {
            error!(
                "Script execution error for rule {}: {}",
                compiled_rule.id, e
            );
            metrics::record_script_execution(&compiled_rule.id, script_duration, "error");
            metrics::record_script_error(&compiled_rule.id, "runtime");

            // Forward request on error
            let upstream_url = selected_upstream_url.unwrap_or(ctx.upstream_uri);
            let response = forward_request_with_body(
                ctx.http_client,
                method.clone(),
                uri.clone(),
                headers.clone(),
                body_bytes,
                upstream_url,
            )
            .await;
            let status = response.status().as_u16();
            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), duration_ms, "none");
            metrics::record_request(method.as_str(), status);
            response.into_boxed()
        }
    }
}

/// Handle a matched YAML rule - returns response or the request back if no fault.
#[allow(clippy::too_many_arguments)]
async fn handle_yaml_rule(
    ctx: &RequestHandlerContext<'_>,
    rule: &CompiledRule,
    req: Request<hyper::body::Incoming>,
    method: &hyper::Method,
    uri: &hyper::Uri,
    headers: &hyper::HeaderMap,
    selected_upstream_url: Option<&str>,
    start_time: std::time::Instant,
) -> RuleHandlingResult {
    // Decide fault
    let fault_decision = decide_fault(&rule.rule.fault, &rule.id);

    match fault_decision {
        FaultDecision::TcpFault {
            fault_type,
            rule_id,
        } => {
            warn!("Injecting TCP fault: {:?}, rule={}", fault_type, rule_id);

            // Record metrics
            metrics::record_error_injection(&rule_id, 0);
            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), duration_ms, "tcp_fault");

            // Return appropriate error based on fault type
            let (status, body) = match fault_type {
                TcpFault::ConnectionResetByPeer => {
                    (502, r#"{"error": "Connection reset by peer"}"#.to_string())
                }
                TcpFault::RandomDataThenClose => (
                    502,
                    r#"{"error": "Connection closed unexpectedly"}"#.to_string(),
                ),
            };

            let mut response = create_error_response(status, body, None, None).unwrap();
            response.set_header(&X_RIFT_FAULT, &VALUE_TCP);
            response.set_header_value(&X_RIFT_RULE_ID, &rule_id);
            response.set_header_value(&X_RIFT_TCP_FAULT, &format!("{fault_type:?}").to_lowercase());
            RuleHandlingResult::Response(response.into_boxed())
        }
        FaultDecision::Error {
            status,
            body,
            rule_id,
            headers: fault_headers,
            behaviors,
        } => {
            warn!("Injecting error fault: status={}, rule={}", status, rule_id);

            // Apply wait behavior if present (Mountebank-compatible)
            if let Some(ref bhvs) = behaviors {
                if let Some(ref wait) = bhvs.wait {
                    let wait_ms = wait.get_duration_ms();
                    debug!("Applying wait behavior: {}ms", wait_ms);
                    apply_latency(wait_ms).await;
                }
            }

            // Record metrics
            metrics::record_error_injection(&rule_id, status);
            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), duration_ms, "error");
            metrics::record_request(method.as_str(), status);

            // Build request context for behaviors
            let request_context = RequestContext::from_request(
                method.as_str(),
                uri,
                headers,
                None, // Body not available for YAML rules
            );

            // Process template variables in response body if present
            let mut processed_body = if has_template_variables(&body) {
                let request_data =
                    RequestData::new(method.as_str(), uri.path(), uri.query(), headers, None);
                process_template(&body, &request_data)
            } else {
                body
            };

            // Clone headers for mutation
            let mut response_headers = fault_headers.clone();

            // Apply copy behaviors (Mountebank-compatible)
            if let Some(ref bhvs) = behaviors {
                if !bhvs.copy.is_empty() {
                    debug!("Applying {} copy behaviors", bhvs.copy.len());
                    processed_body = apply_copy_behaviors(
                        &processed_body,
                        &mut response_headers,
                        &bhvs.copy,
                        &request_context,
                    );
                }
            }

            // Apply lookup behaviors (Mountebank-compatible)
            if let Some(ref bhvs) = behaviors {
                if !bhvs.lookup.is_empty() {
                    debug!("Applying {} lookup behaviors", bhvs.lookup.len());
                    processed_body = apply_lookup_behaviors(
                        &processed_body,
                        &mut response_headers,
                        &bhvs.lookup,
                        &request_context,
                        ctx.csv_cache,
                    );
                }
            }

            // Apply shell transform (Mountebank-compatible)
            if let Some(ref bhvs) = behaviors {
                if let Some(ref cmd) = bhvs.shell_transform {
                    debug!("Applying shell transform: {}", cmd);
                    match apply_shell_transform(cmd, &request_context, &processed_body, status) {
                        Ok(transformed) => {
                            processed_body = transformed;
                        }
                        Err(e) => {
                            warn!("Shell transform failed: {}", e);
                        }
                    }
                }
            }

            // Apply decorate behavior (Mountebank-compatible Rhai script)
            let mut final_status = status;
            if let Some(ref bhvs) = behaviors {
                if let Some(ref script) = bhvs.decorate {
                    debug!("Applying decorate behavior");
                    match apply_decorate(
                        script,
                        &request_context,
                        &processed_body,
                        status,
                        &mut response_headers,
                    ) {
                        Ok((new_body, new_status)) => {
                            processed_body = new_body;
                            final_status = new_status;
                        }
                        Err(e) => {
                            warn!("Decorate behavior failed: {}", e);
                        }
                    }
                }
            }

            let mut response =
                create_error_response(final_status, processed_body, Some(&response_headers), None)
                    .unwrap();
            response.set_header(&X_RIFT_FAULT, &VALUE_ERROR);
            response.set_header_value(&X_RIFT_RULE_ID, &rule_id);

            // Add behavior headers for debugging/testing
            if let Some(ref bhvs) = behaviors {
                if bhvs.wait.is_some() {
                    response.set_header(&X_RIFT_BEHAVIOR_WAIT, &VALUE_TRUE);
                }
                if !bhvs.copy.is_empty() {
                    response.set_header(&X_RIFT_BEHAVIOR_COPY, &VALUE_TRUE);
                }
                if !bhvs.lookup.is_empty() {
                    response.set_header(&X_RIFT_BEHAVIOR_LOOKUP, &VALUE_TRUE);
                }
                if bhvs.shell_transform.is_some() {
                    response.set_header(&X_RIFT_BEHAVIOR_SHELL, &VALUE_TRUE);
                }
                if bhvs.decorate.is_some() {
                    response.set_header(&X_RIFT_BEHAVIOR_DECORATE, &VALUE_TRUE);
                }
            }

            RuleHandlingResult::Response(response.into_boxed())
        }
        FaultDecision::Latency {
            duration_ms,
            rule_id,
        } => {
            info!(
                "Injecting latency fault: {}ms, rule={}",
                duration_ms, rule_id
            );

            // Record metrics
            metrics::record_latency_injection(&rule_id, duration_ms);

            apply_latency(duration_ms).await;

            // Collect body for retry capability
            let body_bytes = match req.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    error!("Failed to collect request body: {}", e);
                    let mut response = error_response(500, "Failed to read request body");
                    response.set_header(&X_RIFT_FAULT, &VALUE_LATENCY);
                    response.set_header_value(&X_RIFT_RULE_ID, &rule_id);
                    return RuleHandlingResult::Response(response.into_boxed());
                }
            };

            // Forward request with latency header
            let upstream_url = selected_upstream_url.unwrap_or(ctx.upstream_uri);
            let mut response = forward_request_with_body(
                ctx.http_client,
                method.clone(),
                uri.clone(),
                headers.clone(),
                body_bytes,
                upstream_url,
            )
            .await;
            let status = response.status().as_u16();
            let total_duration = start_time.elapsed().as_secs_f64() * 1000.0;
            metrics::record_proxy_duration(method.as_str(), total_duration, "latency");
            metrics::record_request(method.as_str(), status);

            response.set_header(&X_RIFT_FAULT, &VALUE_LATENCY);
            response.set_header_value(&X_RIFT_RULE_ID, &rule_id);
            response.set_header_value(&X_RIFT_LATENCY_MS, &duration_ms.to_string());
            RuleHandlingResult::Response(response.into_boxed())
        }
        FaultDecision::None => {
            debug!("No fault injected for matched rule: {}", rule.id);
            RuleHandlingResult::NoFault(req)
        }
    }
}

/// Select upstream for the request based on routing rules.
/// Returns the upstream URL and name if matched, None for sidecar mode.
fn select_upstream<B>(
    router: Option<&Router>,
    upstreams: &[crate::config::Upstream],
    req: &Request<B>,
) -> Option<(String, String)> {
    // If no router configured, use sidecar mode (return None)
    let router = router?;

    // Match request to an upstream name
    let upstream_name = router.match_request(req)?;

    // Find upstream by name
    let upstream = upstreams.iter().find(|u| u.name == upstream_name)?;
    debug!("Routed to upstream: {} ({})", upstream_name, upstream.url);
    Some((upstream.url.clone(), upstream_name.to_string()))
}

/// Check if a rule applies to the given upstream.
/// Returns true if:
/// - Rule has no upstream filter (applies to all)
/// - Rule's upstream matches the selected upstream name
/// - No upstream is selected (sidecar mode - applies to all)
pub fn rule_applies_to_upstream(
    rule_upstream_filter: &Option<String>,
    selected_upstream_name: Option<&str>,
) -> bool {
    match (rule_upstream_filter, selected_upstream_name) {
        // Rule has no filter - applies to all upstreams
        (None, _) => true,
        // No upstream selected (sidecar mode) - rule applies
        (Some(_), None) => true,
        // Both specified - must match
        (Some(rule_upstream), Some(selected)) => rule_upstream == selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_applies_to_upstream_no_filter() {
        // Rule with no upstream filter should apply to all upstreams
        assert!(rule_applies_to_upstream(&None, None));
        assert!(rule_applies_to_upstream(&None, Some("backend-a")));
        assert!(rule_applies_to_upstream(&None, Some("backend-b")));
    }

    #[test]
    fn test_rule_applies_to_upstream_sidecar_mode() {
        // Sidecar mode (no upstream selected) - rule should apply
        assert!(rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            None
        ));
        assert!(rule_applies_to_upstream(
            &Some("backend-b".to_string()),
            None
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_matching() {
        // Rule upstream matches selected upstream
        assert!(rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            Some("backend-a")
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_non_matching() {
        // Rule upstream does NOT match selected upstream
        assert!(!rule_applies_to_upstream(
            &Some("backend-a".to_string()),
            Some("backend-b")
        ));
        assert!(!rule_applies_to_upstream(
            &Some("backend-x".to_string()),
            Some("backend-y")
        ));
    }

    #[test]
    fn test_rule_applies_to_upstream_empty_strings() {
        // Empty string cases
        assert!(rule_applies_to_upstream(&Some("".to_string()), Some("")));
        assert!(!rule_applies_to_upstream(
            &Some("backend".to_string()),
            Some("")
        ));
    }
}

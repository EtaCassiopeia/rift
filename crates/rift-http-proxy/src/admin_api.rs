//! Admin REST API for Rift proxy and imposter management.
//!
//! This module provides a Mountebank-compatible REST API for:
//! - Creating, deleting, and listing imposters
//! - Managing stubs within imposters
//! - Clearing recorded requests and proxy responses
//! - Health and metrics endpoints
//!
//! The API listens on a configurable port (default: 2525).

use crate::imposter::{ImposterConfig, ImposterError, ImposterManager, Stub};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{debug, error, info};

/// Admin API server for Rift
pub struct AdminApiServer {
    addr: SocketAddr,
    manager: Arc<ImposterManager>,
}

impl AdminApiServer {
    /// Create a new admin API server
    pub fn new(addr: SocketAddr, manager: Arc<ImposterManager>) -> Self {
        Self { addr, manager }
    }

    /// Run the admin API server
    pub async fn run(self) -> Result<(), anyhow::Error> {
        let listener = TcpListener::bind(self.addr).await?;
        info!(
            "Rift Admin API (Mountebank-compatible) listening on http://{}",
            self.addr
        );

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let manager = Arc::clone(&self.manager);

            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let manager = Arc::clone(&manager);
                    async move { handle_admin_request(req, manager).await }
                });

                if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                    debug!("Admin API connection error: {}", e);
                }
            });
        }
    }
}

/// Response types
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImposterSummary {
    port: u16,
    protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    number_of_requests: u64,
}

#[derive(Debug, Serialize)]
struct ListImpostersResponse {
    imposters: Vec<ImposterSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImposterDetail {
    port: u16,
    protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    record_requests: bool,
    number_of_requests: u64,
    stubs: Vec<Stub>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requests: Option<Vec<crate::imposter::RecordedRequest>>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    errors: Vec<ErrorDetail>,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct AddStubRequest {
    #[serde(default)]
    index: Option<usize>,
    stub: Stub,
}

/// Main request handler
async fn handle_admin_request(
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(|s| s.to_string());

    debug!("Admin API: {} {}", method, path);

    let response = match (&method, path.as_str()) {
        // Root endpoint
        (&Method::GET, "/") => handle_root(),

        // Imposter endpoints
        (&Method::POST, "/imposters") => handle_create_imposter(req, manager).await,
        (&Method::GET, "/imposters") => handle_list_imposters(manager, query.as_deref()).await,
        (&Method::PUT, "/imposters") => handle_replace_all_imposters(req, manager).await,
        (&Method::DELETE, "/imposters") => handle_delete_all_imposters(manager).await,

        // Individual imposter endpoints
        _ if path.starts_with("/imposters/") => {
            handle_imposter_routes(&method, &path, req, manager).await
        }

        // Health and metrics
        (&Method::GET, "/health") => handle_health(),
        (&Method::GET, "/metrics") => handle_metrics(manager).await,

        // Mountebank-compatible config and logs endpoints
        (&Method::GET, "/config") => handle_config(),
        (&Method::GET, "/logs") => handle_logs(query.as_deref()),

        // Config reload (Rift extension)
        (&Method::POST, "/admin/reload") => handle_reload(),

        // Not found
        _ => not_found(),
    };

    Ok(response)
}

/// Handle imposter-specific routes
async fn handle_imposter_routes(
    method: &Method,
    path: &str,
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    // Parse port from path: /imposters/:port/...
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() < 2 {
        return not_found();
    }

    let port: u16 = match parts[1].parse() {
        Ok(p) => p,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid port number"),
    };

    match (method, parts.as_slice()) {
        // GET /imposters/:port
        (&Method::GET, ["imposters", _]) => handle_get_imposter(port, manager).await,

        // DELETE /imposters/:port
        (&Method::DELETE, ["imposters", _]) => handle_delete_imposter(port, manager).await,

        // POST /imposters/:port/stubs - Add stub
        (&Method::POST, ["imposters", _, "stubs"]) => handle_add_stub(port, req, manager).await,

        // PUT /imposters/:port/stubs - Replace all stubs
        (&Method::PUT, ["imposters", _, "stubs"]) => {
            handle_replace_all_stubs(port, req, manager).await
        }

        // PUT /imposters/:port/stubs/:index - Replace specific stub
        (&Method::PUT, ["imposters", _, "stubs", index_str]) => {
            let index: usize = match index_str.parse() {
                Ok(i) => i,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid stub index"),
            };
            handle_replace_stub(port, index, req, manager).await
        }

        // DELETE /imposters/:port/stubs/:index - Delete specific stub
        (&Method::DELETE, ["imposters", _, "stubs", index_str]) => {
            let index: usize = match index_str.parse() {
                Ok(i) => i,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid stub index"),
            };
            handle_delete_stub(port, index, manager).await
        }

        // DELETE /imposters/:port/savedRequests - Clear recorded requests
        (&Method::DELETE, ["imposters", _, "savedRequests"]) => {
            handle_clear_requests(port, manager).await
        }

        // DELETE /imposters/:port/savedProxyResponses - Clear saved proxy responses
        (&Method::DELETE, ["imposters", _, "savedProxyResponses"]) => {
            handle_clear_proxy_responses(port, manager).await
        }

        // POST /imposters/:port/enable - Enable imposter (Rift extension)
        (&Method::POST, ["imposters", _, "enable"]) => {
            handle_set_enabled(port, true, manager).await
        }

        // POST /imposters/:port/disable - Disable imposter (Rift extension)
        (&Method::POST, ["imposters", _, "disable"]) => {
            handle_set_enabled(port, false, manager).await
        }

        _ => not_found(),
    }
}

/// GET / - Root endpoint
fn handle_root() -> Response<Full<Bytes>> {
    let body = serde_json::json!({
        "name": "rift",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Rift HTTP Proxy with Mountebank-compatible API",
        "_links": {
            "imposters": {"href": "/imposters"},
            "health": {"href": "/health"},
            "metrics": {"href": "/metrics"}
        }
    });
    json_response(StatusCode::OK, &body)
}

/// POST /imposters - Create a new imposter
async fn handle_create_imposter(
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    let mut config: ImposterConfig = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid imposter JSON: {e}"),
            )
        }
    };

    match manager.create_imposter(config.clone()).await {
        Ok(assigned_port) => {
            info!("Created imposter on port {}", assigned_port);
            // Update config with assigned port for response
            config.port = Some(assigned_port);
            json_response(StatusCode::CREATED, &config)
        }
        Err(ImposterError::PortInUse(p)) => error_response(
            StatusCode::BAD_REQUEST,
            &format!("Port {p} is already in use"),
        ),
        Err(ImposterError::InvalidProtocol(p)) => {
            error_response(StatusCode::BAD_REQUEST, &format!("Invalid protocol: {p}"))
        }
        Err(ImposterError::BindError(p, e)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to bind port {p}: {e}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// GET /imposters - List all imposters
async fn handle_list_imposters(
    manager: Arc<ImposterManager>,
    query: Option<&str>,
) -> Response<Full<Bytes>> {
    let replayable = query
        .map(|q| q.contains("replayable=true"))
        .unwrap_or(false);

    let imposters = manager.list_imposters();

    if replayable {
        // Return full imposter configs
        let configs: Vec<ImposterConfig> = imposters.iter().map(|i| i.config.clone()).collect();
        let body = serde_json::json!({ "imposters": configs });
        json_response(StatusCode::OK, &body)
    } else {
        // Return summaries
        let summaries: Vec<ImposterSummary> = imposters
            .iter()
            .filter_map(|i| {
                i.config.port.map(|port| ImposterSummary {
                    port,
                    protocol: i.config.protocol.clone(),
                    name: i.config.name.clone(),
                    number_of_requests: i.get_request_count(),
                })
            })
            .collect();

        let response = ListImpostersResponse {
            imposters: summaries,
        };
        json_response(StatusCode::OK, &response)
    }
}

/// PUT /imposters - Replace all imposters
async fn handle_replace_all_imposters(
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    #[derive(Deserialize)]
    struct BatchRequest {
        imposters: Vec<ImposterConfig>,
    }

    let batch: BatchRequest = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid batch JSON: {e}"))
        }
    };

    // Delete all existing imposters
    manager.delete_all().await;

    // Create new imposters
    let mut created = Vec::new();
    for mut config in batch.imposters {
        match manager.create_imposter(config.clone()).await {
            Ok(assigned_port) => {
                config.port = Some(assigned_port);
                created.push(config);
            }
            Err(e) => {
                error!(
                    "Failed to create imposter on port {:?}: {}",
                    config.port, e
                );
            }
        }
    }

    let body = serde_json::json!({ "imposters": created });
    json_response(StatusCode::OK, &body)
}

/// DELETE /imposters - Delete all imposters
async fn handle_delete_all_imposters(manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
    let configs = manager.delete_all().await;

    let summaries: Vec<ImposterSummary> = configs
        .iter()
        .filter_map(|c| {
            c.port.map(|port| ImposterSummary {
                port,
                protocol: c.protocol.clone(),
                name: c.name.clone(),
                number_of_requests: 0,
            })
        })
        .collect();

    let response = ListImpostersResponse {
        imposters: summaries,
    };
    json_response(StatusCode::OK, &response)
}

/// GET /imposters/:port - Get a specific imposter
async fn handle_get_imposter(port: u16, manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
    match manager.get_imposter(port) {
        Ok(imposter) => {
            let detail = ImposterDetail {
                port: imposter.config.port.unwrap_or(port),
                protocol: imposter.config.protocol.clone(),
                name: imposter.config.name.clone(),
                record_requests: imposter.config.record_requests,
                number_of_requests: imposter.get_request_count(),
                stubs: imposter.get_stubs(), // Use runtime stubs, not config
                requests: if imposter.config.record_requests {
                    Some(imposter.get_recorded_requests())
                } else {
                    None
                },
            };
            json_response(StatusCode::OK, &detail)
        }
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// DELETE /imposters/:port - Delete a specific imposter
/// Returns 200 even if imposter doesn't exist (idempotent delete, matches Mountebank)
async fn handle_delete_imposter(port: u16, manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
    match manager.delete_imposter(port).await {
        Ok(config) => {
            info!("Deleted imposter on port {}", port);
            json_response(StatusCode::OK, &config)
        }
        Err(ImposterError::NotFound(_)) => {
            // Return 200 with empty object for idempotent delete (matches Mountebank)
            json_response(StatusCode::OK, &serde_json::json!({}))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// POST /imposters/:port/stubs - Add a stub
async fn handle_add_stub(
    port: u16,
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    let add_req: AddStubRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid stub JSON: {e}"))
        }
    };

    match manager.add_stub(port, add_req.stub, add_req.index) {
        Ok(()) => {
            // Return updated imposter
            handle_get_imposter(port, manager).await
        }
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// PUT /imposters/:port/stubs - Replace all stubs
async fn handle_replace_all_stubs(
    port: u16,
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    #[derive(Deserialize)]
    struct ReplaceStubsRequest {
        stubs: Vec<Stub>,
    }

    let replace_req: ReplaceStubsRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid stubs JSON: {e}"))
        }
    };

    let imposter = match manager.get_imposter(port) {
        Ok(i) => i,
        Err(ImposterError::NotFound(_)) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Imposter not found on port {port}"),
            )
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    // Clear and add new stubs
    {
        let mut stubs = imposter.stubs.write();
        stubs.clear();
        for stub in replace_req.stubs {
            stubs.push(stub);
        }
    }

    handle_get_imposter(port, manager).await
}

/// PUT /imposters/:port/stubs/:index - Replace a specific stub
async fn handle_replace_stub(
    port: u16,
    index: usize,
    req: Request<Incoming>,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    let stub: Stub = match serde_json::from_slice(&body) {
        Ok(s) => s,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid stub JSON: {e}"))
        }
    };

    match manager.replace_stub(port, index, stub) {
        Ok(()) => handle_get_imposter(port, manager).await,
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(ImposterError::StubIndexOutOfBounds(i)) => {
            error_response(StatusCode::NOT_FOUND, &format!("Stub index {i} not found"))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// DELETE /imposters/:port/stubs/:index - Delete a specific stub
async fn handle_delete_stub(
    port: u16,
    index: usize,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.delete_stub(port, index) {
        Ok(()) => handle_get_imposter(port, manager).await,
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(ImposterError::StubIndexOutOfBounds(i)) => {
            error_response(StatusCode::NOT_FOUND, &format!("Stub index {i} not found"))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// DELETE /imposters/:port/savedRequests - Clear recorded requests
async fn handle_clear_requests(port: u16, manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
    match manager.get_imposter(port) {
        Ok(imposter) => {
            imposter.clear_recorded_requests();
            json_response(
                StatusCode::OK,
                &serde_json::json!({"message": "Requests cleared"}),
            )
        }
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// POST /imposters/:port/enable or /disable - Set enabled state
async fn handle_set_enabled(
    port: u16,
    enabled: bool,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.get_imposter(port) {
        Ok(imposter) => {
            imposter.set_enabled(enabled);
            let state = if enabled { "enabled" } else { "disabled" };
            json_response(
                StatusCode::OK,
                &serde_json::json!({"message": format!("Imposter {}", state)}),
            )
        }
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// GET /health - Health check
fn handle_health() -> Response<Full<Bytes>> {
    json_response(StatusCode::OK, &serde_json::json!({"status": "ok"}))
}

/// GET /metrics - Prometheus metrics
async fn handle_metrics(manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
    let imposters = manager.list_imposters();

    let mut metrics = String::new();
    metrics.push_str("# HELP rift_imposters_total Total number of active imposters\n");
    metrics.push_str("# TYPE rift_imposters_total gauge\n");
    metrics.push_str(&format!("rift_imposters_total {}\n", imposters.len()));

    metrics.push_str("# HELP rift_imposter_requests_total Total requests per imposter\n");
    metrics.push_str("# TYPE rift_imposter_requests_total counter\n");
    for imposter in &imposters {
        if let Some(port) = imposter.config.port {
            metrics.push_str(&format!(
                "rift_imposter_requests_total{{port=\"{}\"}} {}\n",
                port,
                imposter.get_request_count()
            ));
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(Full::new(Bytes::from(metrics)))
        .unwrap()
}

/// POST /admin/reload - Reload configuration (Rift extension)
fn handle_reload() -> Response<Full<Bytes>> {
    // This would trigger a config reload in a real implementation
    json_response(
        StatusCode::OK,
        &serde_json::json!({"message": "Reload not implemented yet"}),
    )
}

/// GET /config - Mountebank-compatible config endpoint
fn handle_config() -> Response<Full<Bytes>> {
    let config = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "options": {
            "port": 2525,
            "allowInjection": std::env::var("MB_ALLOW_INJECTION")
                .map(|v| v == "true")
                .unwrap_or(false),
            "localOnly": false,
            "ipWhitelist": ["*"]
        },
        "process": {
            "nodeVersion": "N/A (Rust)",
            "architecture": std::env::consts::ARCH,
            "platform": std::env::consts::OS,
            "rss": 0,
            "heapTotal": 0,
            "heapUsed": 0,
            "uptime": 0,
            "cwd": std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        }
    });
    json_response(StatusCode::OK, &config)
}

/// GET /logs - Mountebank-compatible logs endpoint
fn handle_logs(query: Option<&str>) -> Response<Full<Bytes>> {
    // Parse query parameters for pagination
    let mut start_index = 0;
    let mut end_index = 100;

    if let Some(q) = query {
        for param in q.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                match key {
                    "startIndex" => {
                        if let Ok(v) = value.parse::<usize>() {
                            start_index = v;
                        }
                    }
                    "endIndex" => {
                        if let Ok(v) = value.parse::<usize>() {
                            end_index = v;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Rift doesn't store logs in memory like Mountebank, so return empty array
    // with the requested pagination info
    let logs = serde_json::json!({
        "logs": [],
        "_links": {
            "self": {
                "href": format!("/logs?startIndex={}&endIndex={}", start_index, end_index)
            }
        }
    });
    json_response(StatusCode::OK, &logs)
}

/// DELETE /imposters/:port/savedProxyResponses - Clear saved proxy responses
async fn handle_clear_proxy_responses(
    port: u16,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.get_imposter(port) {
        Ok(imposter) => {
            imposter.clear_proxy_responses();
            json_response(
                StatusCode::OK,
                &serde_json::json!({"message": "Saved proxy responses cleared"}),
            )
        }
        Err(ImposterError::NotFound(_)) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Imposter not found on port {port}"),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Helper: Collect request body
async fn collect_body(req: Request<Incoming>) -> Result<Bytes, String> {
    req.collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| format!("Failed to read request body: {e}"))
}

/// Helper: JSON response
fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<Full<Bytes>> {
    let json = serde_json::to_string_pretty(body).unwrap_or_else(|_| "{}".to_string());
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))
        .unwrap()
}

/// Helper: Error response
fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let error = ErrorResponse {
        errors: vec![ErrorDetail {
            code: status.as_str().to_string(),
            message: message.to_string(),
        }],
    };
    json_response(status, &error)
}

/// Helper: Not found response
fn not_found() -> Response<Full<Bytes>> {
    error_response(StatusCode::NOT_FOUND, "Not Found")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // Tests for helper functions
    // ============================================

    #[test]
    fn test_error_response_format() {
        let resp = error_response(StatusCode::BAD_REQUEST, "Test error");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_json_response() {
        let body = serde_json::json!({"test": "value"});
        let resp = json_response(StatusCode::OK, &body);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("Content-Type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_not_found_response() {
        let resp = not_found();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_response_various_codes() {
        let codes = vec![
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
        ];

        for code in codes {
            let resp = error_response(code, "Test message");
            assert_eq!(resp.status(), code);
            assert_eq!(
                resp.headers().get("Content-Type").unwrap(),
                "application/json"
            );
        }
    }

    #[test]
    fn test_json_response_with_complex_body() {
        let body = serde_json::json!({
            "nested": {
                "array": [1, 2, 3],
                "object": {"key": "value"}
            },
            "number": 42,
            "boolean": true,
            "null_value": null
        });
        let resp = json_response(StatusCode::OK, &body);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ============================================
    // Tests for response types serialization
    // ============================================

    #[test]
    fn test_imposter_summary_serialization() {
        let summary = ImposterSummary {
            port: 8080,
            protocol: "http".to_string(),
            name: Some("test-imposter".to_string()),
            number_of_requests: 42,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"port\":8080"));
        assert!(json.contains("\"protocol\":\"http\""));
        assert!(json.contains("\"name\":\"test-imposter\""));
        assert!(json.contains("\"numberOfRequests\":42"));
    }

    #[test]
    fn test_imposter_summary_without_name() {
        let summary = ImposterSummary {
            port: 3000,
            protocol: "https".to_string(),
            name: None,
            number_of_requests: 0,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"port\":3000"));
        assert!(!json.contains("\"name\"")); // name should be skipped when None
    }

    #[test]
    fn test_list_imposters_response_serialization() {
        let response = ListImpostersResponse {
            imposters: vec![
                ImposterSummary {
                    port: 8080,
                    protocol: "http".to_string(),
                    name: None,
                    number_of_requests: 10,
                },
                ImposterSummary {
                    port: 8443,
                    protocol: "https".to_string(),
                    name: Some("secure".to_string()),
                    number_of_requests: 20,
                },
            ],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"imposters\""));
        assert!(json.contains("8080"));
        assert!(json.contains("8443"));
    }

    #[test]
    fn test_error_response_serialization() {
        let error = ErrorResponse {
            errors: vec![ErrorDetail {
                code: "400".to_string(),
                message: "Invalid request".to_string(),
            }],
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"errors\""));
        assert!(json.contains("\"code\":\"400\""));
        assert!(json.contains("\"message\":\"Invalid request\""));
    }

    #[test]
    fn test_error_response_multiple_errors() {
        let error = ErrorResponse {
            errors: vec![
                ErrorDetail {
                    code: "validation_error".to_string(),
                    message: "Field 'port' is required".to_string(),
                },
                ErrorDetail {
                    code: "validation_error".to_string(),
                    message: "Field 'protocol' must be 'http' or 'https'".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"errors\""));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_add_stub_request_deserialization() {
        let json = r#"{"index": 0, "stub": {"responses": [{"is": {"statusCode": 200}}]}}"#;
        let req: AddStubRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.index, Some(0));
    }

    #[test]
    fn test_add_stub_request_without_index() {
        let json = r#"{"stub": {"responses": [{"is": {"statusCode": 200}}]}}"#;
        let req: AddStubRequest = serde_json::from_str(json).unwrap();
        assert!(req.index.is_none());
    }

    // ============================================
    // Tests for handle_root
    // ============================================

    #[test]
    fn test_handle_root() {
        let resp = handle_root();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("Content-Type").unwrap(),
            "application/json"
        );
    }

    // ============================================
    // Tests for handle_health
    // ============================================

    #[test]
    fn test_handle_health() {
        let resp = handle_health();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ============================================
    // Tests for handle_config
    // ============================================

    #[test]
    fn test_handle_config() {
        let resp = handle_config();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ============================================
    // Tests for handle_reload
    // ============================================

    #[test]
    fn test_handle_reload() {
        let resp = handle_reload();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ============================================
    // Tests for handle_logs
    // ============================================

    #[test]
    fn test_handle_logs_no_query() {
        let resp = handle_logs(None);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_logs_with_pagination() {
        let resp = handle_logs(Some("startIndex=10&endIndex=50"));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_logs_invalid_pagination() {
        // Invalid values should be ignored, defaults used
        let resp = handle_logs(Some("startIndex=invalid&endIndex=also_invalid"));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_logs_partial_pagination() {
        let resp = handle_logs(Some("startIndex=5"));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ============================================
    // Tests for AdminApiServer creation
    // ============================================

    #[test]
    fn test_admin_api_server_creation() {
        use std::net::{IpAddr, Ipv4Addr};

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2525);
        let manager = Arc::new(ImposterManager::new());
        let server = AdminApiServer::new(addr, manager);
        assert_eq!(server.addr, addr);
    }

    // ============================================
    // Tests for ImposterDetail serialization
    // ============================================

    #[test]
    fn test_imposter_detail_serialization() {
        let detail = ImposterDetail {
            port: 8080,
            protocol: "http".to_string(),
            name: Some("test".to_string()),
            record_requests: true,
            number_of_requests: 100,
            stubs: vec![],
            requests: Some(vec![]),
        };
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains("\"port\":8080"));
        assert!(json.contains("\"recordRequests\":true"));
        assert!(json.contains("\"numberOfRequests\":100"));
    }

    #[test]
    fn test_imposter_detail_without_requests() {
        let detail = ImposterDetail {
            port: 8080,
            protocol: "http".to_string(),
            name: None,
            record_requests: false,
            number_of_requests: 0,
            stubs: vec![],
            requests: None,
        };
        let json = serde_json::to_string(&detail).unwrap();
        // requests should be skipped when None
        assert!(!json.contains("\"requests\""));
    }

    // ============================================
    // Integration tests for route parsing
    // ============================================

    mod route_parsing_tests {
        #[test]
        fn test_path_parsing_imposters_port() {
            let path = "/imposters/8080";
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            assert_eq!(parts, vec!["imposters", "8080"]);
            let port: u16 = parts[1].parse().unwrap();
            assert_eq!(port, 8080);
        }

        #[test]
        fn test_path_parsing_imposters_stubs() {
            let path = "/imposters/3000/stubs";
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            assert_eq!(parts, vec!["imposters", "3000", "stubs"]);
        }

        #[test]
        fn test_path_parsing_imposters_stubs_index() {
            let path = "/imposters/3000/stubs/0";
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            assert_eq!(parts, vec!["imposters", "3000", "stubs", "0"]);
            let index: usize = parts[3].parse().unwrap();
            assert_eq!(index, 0);
        }

        #[test]
        fn test_path_parsing_saved_requests() {
            let path = "/imposters/8080/savedRequests";
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            assert_eq!(parts, vec!["imposters", "8080", "savedRequests"]);
        }

        #[test]
        fn test_invalid_port_parsing() {
            let invalid_port = "not_a_number";
            let result: Result<u16, _> = invalid_port.parse();
            assert!(result.is_err());
        }

        #[test]
        fn test_invalid_index_parsing() {
            let invalid_index = "abc";
            let result: Result<usize, _> = invalid_index.parse();
            assert!(result.is_err());
        }
    }

    // ============================================
    // Tests for query parameter parsing
    // ============================================

    mod query_parsing_tests {
        #[test]
        fn test_replayable_query_true() {
            let query = Some("replayable=true");
            let replayable = query
                .map(|q| q.contains("replayable=true"))
                .unwrap_or(false);
            assert!(replayable);
        }

        #[test]
        fn test_replayable_query_false() {
            let query = Some("replayable=false");
            let replayable = query
                .map(|q| q.contains("replayable=true"))
                .unwrap_or(false);
            assert!(!replayable);
        }

        #[test]
        fn test_replayable_query_missing() {
            let query: Option<&str> = None;
            let replayable = query
                .map(|q| q.contains("replayable=true"))
                .unwrap_or(false);
            assert!(!replayable);
        }

        #[test]
        fn test_replayable_with_other_params() {
            let query = Some("format=json&replayable=true&pretty=1");
            let replayable = query
                .map(|q| q.contains("replayable=true"))
                .unwrap_or(false);
            assert!(replayable);
        }
    }
}

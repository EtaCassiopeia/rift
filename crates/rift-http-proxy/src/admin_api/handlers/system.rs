//! System handlers: health, metrics, config, logs.

use crate::admin_api::types::*;
use crate::imposter::ImposterManager;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use std::sync::Arc;

/// GET / - Root endpoint (Mountebank-compatible format)
pub fn handle_root(base_url: &str) -> Response<Full<Bytes>> {
    let body = serde_json::json!({
        "_links": {
            "imposters": {"href": format!("{}/imposters", base_url)},
            "config": {"href": format!("{}/config", base_url)},
            "logs": {"href": format!("{}/logs", base_url)}
        }
    });
    json_response(StatusCode::OK, &body)
}

/// GET /health - Health check
pub fn handle_health() -> Response<Full<Bytes>> {
    json_response(StatusCode::OK, &serde_json::json!({"status": "ok"}))
}

/// GET /metrics - Prometheus metrics
pub async fn handle_metrics(manager: Arc<ImposterManager>) -> Response<Full<Bytes>> {
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

/// GET /config - Mountebank-compatible config endpoint
pub fn handle_config() -> Response<Full<Bytes>> {
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
pub fn handle_logs(query: Option<&str>) -> Response<Full<Bytes>> {
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

/// POST /admin/reload - Reload configuration (Rift extension)
pub fn handle_reload() -> Response<Full<Bytes>> {
    json_response(
        StatusCode::OK,
        &serde_json::json!({"message": "Reload not implemented yet"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_root() {
        let resp = handle_root("http://localhost:2525");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_health() {
        let resp = handle_health();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_config() {
        let resp = handle_config();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_handle_reload() {
        let resp = handle_reload();
        assert_eq!(resp.status(), StatusCode::OK);
    }

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
}

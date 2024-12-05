use crate::behaviors::ResponseBehaviors;
use crate::config::{FaultConfig, TcpFault};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Response, StatusCode};
use rand::Rng;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum FaultDecision {
    None,
    Latency {
        duration_ms: u64,
        rule_id: String,
    },
    Error {
        status: u16,
        body: String,
        rule_id: String,
        headers: HashMap<String, String>,
        /// Optional behaviors for response modification (Mountebank-compatible)
        behaviors: Option<ResponseBehaviors>,
    },
    /// TCP-level fault (Mountebank-compatible)
    TcpFault {
        fault_type: TcpFault,
        rule_id: String,
    },
}

pub fn decide_fault(fault_config: &FaultConfig, rule_id: &str) -> FaultDecision {
    let mut rng = rand::thread_rng();

    // Check TCP fault first (highest priority - immediate connection failure)
    if let Some(tcp_fault) = &fault_config.tcp_fault {
        return FaultDecision::TcpFault {
            fault_type: *tcp_fault,
            rule_id: rule_id.to_string(),
        };
    }

    // Check error fault (higher priority than latency)
    if let Some(error_fault) = &fault_config.error {
        if should_inject(error_fault.probability, &mut rng) {
            return FaultDecision::Error {
                status: error_fault.status,
                body: error_fault.body.clone(),
                rule_id: rule_id.to_string(),
                headers: error_fault.headers.clone(),
                behaviors: error_fault.behaviors.clone(),
            };
        }
    }

    // Check latency fault
    if let Some(latency_fault) = &fault_config.latency {
        if should_inject(latency_fault.probability, &mut rng) {
            let duration_ms = rng.gen_range(latency_fault.min_ms..=latency_fault.max_ms);
            return FaultDecision::Latency {
                duration_ms,
                rule_id: rule_id.to_string(),
            };
        }
    }

    FaultDecision::None
}

fn should_inject(probability: f64, rng: &mut impl Rng) -> bool {
    rng.gen::<f64>() < probability
}

pub async fn apply_latency(duration_ms: u64) {
    tokio::time::sleep(Duration::from_millis(duration_ms)).await;
}

/// Create an error response with optional fixed and dynamic headers
/// Dynamic headers override fixed headers when keys conflict
/// Content-Length is always set to actual body length
/// Content-Type defaults to application/json if not provided
pub fn create_error_response(
    status: u16,
    body: String,
    fixed_headers: Option<&HashMap<String, String>>,
    dynamic_headers: Option<&HashMap<String, String>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body_bytes = Bytes::from(body);
    let content_length = body_bytes.len();

    // Merge headers: fixed first, then dynamic (overriding fixed)
    let mut merged = HashMap::new();

    if let Some(fixed) = fixed_headers {
        for (k, v) in fixed {
            merged.insert(k.to_lowercase(), v.clone());
        }
    }

    if let Some(dynamic) = dynamic_headers {
        for (k, v) in dynamic {
            merged.insert(k.to_lowercase(), v.clone());
        }
    }

    // Build response with headers
    let mut builder = Response::builder().status(status_code);

    // Apply merged headers
    let mut has_content_type = false;
    for (key, value) in merged.iter() {
        // Skip transfer-encoding as we're setting content-length
        if key == "transfer-encoding" {
            continue;
        }
        if key == "content-type" {
            has_content_type = true;
        }

        // Try to parse and insert header
        if let (Ok(header_name), Ok(header_value)) = (
            hyper::header::HeaderName::try_from(key.as_str()),
            hyper::header::HeaderValue::from_str(value),
        ) {
            builder = builder.header(header_name, header_value);
        }
    }

    // Set default Content-Type if not provided
    if !has_content_type {
        builder = builder.header("content-type", "application/json");
    }

    // Always set Content-Length to actual body length (overrides any user-provided value)
    builder = builder.header("content-length", content_length.to_string());

    let response = builder.body(Full::new(body_bytes)).unwrap();
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ErrorFault, LatencyFault};

    #[test]
    fn test_should_inject_always() {
        let mut rng = rand::thread_rng();
        let mut count = 0;
        for _ in 0..100 {
            if should_inject(1.0, &mut rng) {
                count += 1;
            }
        }
        assert_eq!(count, 100);
    }

    #[test]
    fn test_should_inject_never() {
        let mut rng = rand::thread_rng();
        let mut count = 0;
        for _ in 0..100 {
            if should_inject(0.0, &mut rng) {
                count += 1;
            }
        }
        assert_eq!(count, 0);
    }

    #[test]
    fn test_should_inject_probability() {
        let mut rng = rand::thread_rng();
        let mut count = 0;
        let iterations = 10000;
        let target_probability = 0.3;

        for _ in 0..iterations {
            if should_inject(target_probability, &mut rng) {
                count += 1;
            }
        }

        let actual_probability = count as f64 / iterations as f64;
        // Allow 5% variance
        assert!(
            (actual_probability - target_probability).abs() < 0.05,
            "Expected ~{target_probability}, got {actual_probability}"
        );
    }

    #[test]
    fn test_decide_fault_with_error() {
        let fault_config = FaultConfig {
            latency: None,
            error: Some(ErrorFault {
                probability: 1.0,
                status: 502,
                body: "error".to_string(),
                headers: HashMap::new(),
                behaviors: None,
            }),
            tcp_fault: None,
        };

        let decision = decide_fault(&fault_config, "test-rule");
        match decision {
            FaultDecision::Error {
                status,
                body,
                rule_id,
                headers,
                ..
            } => {
                assert_eq!(status, 502);
                assert_eq!(body, "error");
                assert_eq!(rule_id, "test-rule");
                assert!(headers.is_empty()); // No headers in this test
            }
            _ => panic!("Expected Error decision"),
        }
    }

    #[test]
    fn test_decide_fault_with_latency() {
        let fault_config = FaultConfig {
            latency: Some(LatencyFault {
                probability: 1.0,
                min_ms: 100,
                max_ms: 200,
            }),
            error: None,
            tcp_fault: None,
        };

        let decision = decide_fault(&fault_config, "test-rule");
        match decision {
            FaultDecision::Latency {
                duration_ms,
                rule_id,
            } => {
                assert!((100..=200).contains(&duration_ms));
                assert_eq!(rule_id, "test-rule");
            }
            _ => panic!("Expected Latency decision"),
        }
    }

    #[test]
    fn test_create_error_response() {
        let response =
            create_error_response(502, r#"{"error": "test"}"#.to_string(), None, None).unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        // Should have default content-type
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_create_error_response_with_headers() {
        let mut fixed_headers = HashMap::new();
        fixed_headers.insert("Server".to_string(), "openresty".to_string());
        fixed_headers.insert("X-Custom".to_string(), "fixed-value".to_string());

        let mut dynamic_headers = HashMap::new();
        dynamic_headers.insert("X-Custom".to_string(), "dynamic-value".to_string());
        dynamic_headers.insert("X-Dynamic".to_string(), "new-header".to_string());

        let response = create_error_response(
            502,
            r#"{"error": "test"}"#.to_string(),
            Some(&fixed_headers),
            Some(&dynamic_headers),
        )
        .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        // Dynamic should override fixed
        assert_eq!(response.headers().get("x-custom").unwrap(), "dynamic-value");
        // Fixed header should be present
        assert_eq!(response.headers().get("server").unwrap(), "openresty");
        // Dynamic-only header should be present
        assert_eq!(response.headers().get("x-dynamic").unwrap(), "new-header");
        // Content-Length should be set
        assert!(response.headers().get("content-length").is_some());
    }
}

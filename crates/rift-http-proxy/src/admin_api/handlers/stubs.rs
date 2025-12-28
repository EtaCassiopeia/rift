//! Stub management handlers.

use crate::admin_api::handlers::imposters::handle_get as handle_get_imposter;
use crate::admin_api::types::{
    collect_body, error_response, imposter_not_found, json_response, make_stub_links,
    AddStubRequest, ReplaceStubsRequest, StubWithLinks,
};
use crate::extensions::stub_analysis::{analyze_new_stub, analyze_stubs};
use crate::imposter::{ImposterError, ImposterManager, Stub};
use crate::scripting::{validate_stub, validate_stubs};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use std::sync::Arc;
use tracing::warn;

/// POST /imposters/:port/stubs - Add a stub
pub async fn handle_add(
    port: u16,
    req: Request<Incoming>,
    base_url: &str,
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

    // Validate scripts in the stub before adding
    let insert_index = add_req.index.unwrap_or(0);
    let validation_result = validate_stub(&add_req.stub, insert_index);
    if !validation_result.is_valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!(
                "Script validation failed: {}",
                validation_result.into_error_message().unwrap_or_default()
            ),
        );
    }

    // Analyze the new stub against existing stubs (Rift extension)
    if let Ok(imposter) = manager.get_imposter(port) {
        let existing_stubs = imposter.get_stubs();
        let insert_index = add_req.index.unwrap_or(existing_stubs.len());
        let analysis = analyze_new_stub(&existing_stubs, &add_req.stub, insert_index);

        for warning in &analysis.warnings {
            warn!(
                port = port,
                stub_id = ?add_req.stub.id,
                warning_type = ?warning.warning_type,
                "New stub warning: {}",
                warning.message
            );
        }
    }

    match manager.add_stub(port, add_req.stub, add_req.index) {
        Ok(()) => handle_get_imposter(port, None, base_url, manager).await,
        Err(ImposterError::NotFound(_)) => imposter_not_found(port),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// PUT /imposters/:port/stubs - Replace all stubs
pub async fn handle_replace_all(
    port: u16,
    req: Request<Incoming>,
    base_url: &str,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    let replace_req: ReplaceStubsRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid stubs JSON: {e}"))
        }
    };

    // Validate all scripts in stubs before replacing
    let validation_result = validate_stubs(&replace_req.stubs);
    if !validation_result.is_valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!(
                "Script validation failed: {}",
                validation_result.into_error_message().unwrap_or_default()
            ),
        );
    }

    // Analyze the new stubs (Rift extension)
    let analysis = analyze_stubs(&replace_req.stubs);
    for warning in &analysis.warnings {
        warn!(
            port = port,
            warning_type = ?warning.warning_type,
            "Stub replacement warning: {}",
            warning.message
        );
    }

    let imposter = match manager.get_imposter(port) {
        Ok(i) => i,
        Err(ImposterError::NotFound(_)) => return imposter_not_found(port),
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    {
        let mut stubs = imposter.stubs.write();
        stubs.clear();
        for stub in replace_req.stubs {
            stubs.push(stub);
        }
    }

    handle_get_imposter(port, None, base_url, manager).await
}

/// GET /imposters/:port/stubs - Get all stubs
pub async fn handle_get_all(
    port: u16,
    base_url: &str,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.get_imposter(port) {
        Ok(imposter) => {
            let stubs = imposter.get_stubs();
            let stubs_with_links: Vec<StubWithLinks> = stubs
                .into_iter()
                .enumerate()
                .map(|(index, stub)| StubWithLinks {
                    stub,
                    links: make_stub_links(base_url, port, index),
                })
                .collect();
            json_response(
                StatusCode::OK,
                &serde_json::json!({ "stubs": stubs_with_links }),
            )
        }
        Err(ImposterError::NotFound(_)) => imposter_not_found(port),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// GET /imposters/:port/stubs/:index - Get a specific stub
pub async fn handle_get(
    port: u16,
    index: usize,
    base_url: &str,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.get_stub(port, index) {
        Ok(stub) => {
            let stub_with_links = StubWithLinks {
                stub,
                links: make_stub_links(base_url, port, index),
            };
            json_response(StatusCode::OK, &stub_with_links)
        }
        Err(ImposterError::NotFound(_)) => imposter_not_found(port),
        Err(ImposterError::StubIndexOutOfBounds(i)) => {
            error_response(StatusCode::NOT_FOUND, &format!("Stub index {i} not found"))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// PUT /imposters/:port/stubs/:index - Replace a specific stub
pub async fn handle_replace(
    port: u16,
    index: usize,
    req: Request<Incoming>,
    base_url: &str,
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

    // Validate scripts in the stub before replacing
    let validation_result = validate_stub(&stub, index);
    if !validation_result.is_valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!(
                "Script validation failed: {}",
                validation_result.into_error_message().unwrap_or_default()
            ),
        );
    }

    match manager.replace_stub(port, index, stub) {
        Ok(()) => handle_get_imposter(port, None, base_url, manager).await,
        Err(ImposterError::NotFound(_)) => imposter_not_found(port),
        Err(ImposterError::StubIndexOutOfBounds(i)) => {
            error_response(StatusCode::NOT_FOUND, &format!("Stub index {i} not found"))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// DELETE /imposters/:port/stubs/:index - Delete a specific stub
pub async fn handle_delete(
    port: u16,
    index: usize,
    base_url: &str,
    manager: Arc<ImposterManager>,
) -> Response<Full<Bytes>> {
    match manager.delete_stub(port, index) {
        Ok(()) => handle_get_imposter(port, None, base_url, manager).await,
        Err(ImposterError::NotFound(_)) => imposter_not_found(port),
        Err(ImposterError::StubIndexOutOfBounds(i)) => {
            error_response(StatusCode::NOT_FOUND, &format!("Stub index {i} not found"))
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

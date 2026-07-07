//! Intercept rule CRUD + CA/truststore export admin handlers (epic #394, slice 4/5).
//!
//! Everything here lives under `/intercept/...` and is only reachable when the server was built
//! `with_intercept(...)` (i.e. the intercept listener is actually running) — see
//! `admin_api::router::route_request`.

use crate::admin_api::types::{collect_body, error_response, json_response};
use crate::intercept_rules::{InterceptRule, InterceptState};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use rift_core::proxy::truststore::{TrustStorePassword, ca_pem, export_jks, export_pkcs12};
use serde::Serialize;

const DEFAULT_TRUSTSTORE_PASSWORD: &str = "changeit";

/// Dispatch a `/intercept/...` admin request. Returns `None` for any other path so the caller
/// falls through to its normal routing (e.g. `404`).
pub async fn route(
    method: &Method,
    path: &str,
    query: Option<&str>,
    req: Request<Incoming>,
    state: &InterceptState,
) -> Option<Response<Full<Bytes>>> {
    let rest = path.strip_prefix("/intercept")?;
    let resp = match (method, rest) {
        (&Method::POST, "/rules") => handle_add_rules(req, state).await,
        (&Method::GET, "/rules") => handle_list_rules(state),
        (&Method::DELETE, "/rules") => handle_clear_rules(state),
        (&Method::GET, "/ca.pem") => handle_ca_pem(state),
        (&Method::GET, "/truststore.p12") => handle_truststore_p12(query, state),
        (&Method::GET, "/truststore.jks") => handle_truststore_jks(query, state),
        // Unmatched `/intercept/...` sub-path: let the caller apply its own 404 handling.
        _ => return None,
    };
    Some(resp)
}

/// A single rule or a batch — `POST /intercept/rules` accepts either shape.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum RuleOrRules {
    One(InterceptRule),
    Many(Vec<InterceptRule>),
}

/// `POST /intercept/rules` — add one rule (a bare `InterceptRule` object) or many (a JSON array).
async fn handle_add_rules(req: Request<Incoming>, state: &InterceptState) -> Response<Full<Bytes>> {
    let body = match collect_body(req).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    add_rules_from_bytes(&body, state)
}

/// Parse a rule (or array of rules) from a JSON body and add them to the store. Split out from
/// `handle_add_rules` so the parse/store path is unit-testable without a `Request<Incoming>`.
fn add_rules_from_bytes(body: &[u8], state: &InterceptState) -> Response<Full<Bytes>> {
    let parsed: RuleOrRules = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid intercept rule JSON: {e}"),
            );
        }
    };
    let added = match parsed {
        RuleOrRules::One(rule) => {
            state.rules.add(rule.clone());
            vec![rule]
        }
        RuleOrRules::Many(rules) => {
            for rule in &rules {
                state.rules.add(rule.clone());
            }
            rules
        }
    };
    json_response(StatusCode::CREATED, &added)
}

/// `GET /intercept/rules` — list all current rules.
fn handle_list_rules(state: &InterceptState) -> Response<Full<Bytes>> {
    json_response(StatusCode::OK, &state.rules.list())
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct DeletedResponse {
    deleted: usize,
}

/// `DELETE /intercept/rules` — remove all rules, returning how many were removed.
fn handle_clear_rules(state: &InterceptState) -> Response<Full<Bytes>> {
    let deleted = state.rules.len();
    state.rules.clear();
    json_response(StatusCode::OK, &DeletedResponse { deleted })
}

/// `GET /intercept/ca.pem` — the intercept CA certificate, PEM-encoded.
fn handle_ca_pem(state: &InterceptState) -> Response<Full<Bytes>> {
    let pem = ca_pem(&state.ca);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-pem-file")
        .body(Full::new(Bytes::from(pem)))
        .unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to build response",
            )
        })
}

/// Extract `password=` from a query string, defaulting to [`DEFAULT_TRUSTSTORE_PASSWORD`].
fn password_from_query(query: Option<&str>) -> String {
    query
        .and_then(|q| {
            q.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "password").then(|| {
                    urlencoding::decode(v)
                        .map(|d| d.into_owned())
                        .unwrap_or_else(|_| v.to_string())
                })
            })
        })
        .unwrap_or_else(|| DEFAULT_TRUSTSTORE_PASSWORD.to_string())
}

fn truststore_response(bytes: Vec<u8>, password: &str, filename: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .header("x-truststore-password", password)
        .body(Full::new(Bytes::from(bytes)))
        .unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to build response",
            )
        })
}

/// `GET /intercept/truststore.p12[?password=]` — PKCS#12 truststore containing the CA cert.
fn handle_truststore_p12(query: Option<&str>, state: &InterceptState) -> Response<Full<Bytes>> {
    let password = password_from_query(query);
    match export_pkcs12(&state.ca, &TrustStorePassword::new(password.clone())) {
        Ok(bytes) => truststore_response(bytes, &password, "rift-intercept-ca.p12"),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to export PKCS#12 truststore: {e}"),
        ),
    }
}

/// `GET /intercept/truststore.jks[?password=]` — JKS truststore containing the CA cert.
fn handle_truststore_jks(query: Option<&str>, state: &InterceptState) -> Response<Full<Bytes>> {
    let password = password_from_query(query);
    match export_jks(&state.ca, &TrustStorePassword::new(password.clone())) {
        Ok(bytes) => truststore_response(bytes, &password, "rift-intercept-ca.jks"),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to export JKS truststore: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intercept_rules::{InterceptAction, InterceptRules, ServeStub};
    use rift_core::proxy::intercept_ca::CertificateAuthority;
    use std::sync::Arc;

    fn test_state() -> InterceptState {
        InterceptState {
            rules: InterceptRules::new(),
            ca: Arc::new(CertificateAuthority::generate().expect("generate CA")),
        }
    }

    #[test]
    fn ca_and_truststore_export_handlers() {
        let state = test_state();

        let ca_resp = handle_ca_pem(&state);
        assert_eq!(ca_resp.status(), StatusCode::OK);
        let ca_body = body_string(ca_resp);
        assert!(ca_body.starts_with("-----BEGIN CERTIFICATE-----"));

        let p12_resp = handle_truststore_p12(Some("password=hunter2"), &state);
        assert_eq!(p12_resp.status(), StatusCode::OK);
        assert_eq!(
            p12_resp
                .headers()
                .get("x-truststore-password")
                .unwrap()
                .to_str()
                .unwrap(),
            "hunter2"
        );
        // `p12` is not a direct dependency of this crate, so we assert non-empty bytes + the
        // password header rather than round-tripping the parser (rift-core's own tests already
        // cover the PKCS#12 encoding itself).
        assert!(!body_bytes(p12_resp).is_empty());

        let jks_resp = handle_truststore_jks(None, &state);
        assert_eq!(jks_resp.status(), StatusCode::OK);
        assert_eq!(
            jks_resp
                .headers()
                .get("x-truststore-password")
                .unwrap()
                .to_str()
                .unwrap(),
            DEFAULT_TRUSTSTORE_PASSWORD
        );
        assert!(!body_bytes(jks_resp).is_empty());
    }

    #[test]
    fn rules_endpoints_list_and_clear() {
        let state = test_state();
        state.rules.add(InterceptRule {
            host: None,
            predicates: vec![],
            action: InterceptAction::Serve(ServeStub {
                status_code: 200,
                headers: Default::default(),
                body: None,
            }),
        });

        let list_resp = handle_list_rules(&state);
        assert_eq!(list_resp.status(), StatusCode::OK);
        let listed: Vec<InterceptRule> = serde_json::from_str(&body_string(list_resp)).unwrap();
        assert_eq!(listed.len(), 1);

        let clear_resp = handle_clear_rules(&state);
        assert_eq!(clear_resp.status(), StatusCode::OK);
        let deleted: DeletedResponse = serde_json::from_str(&body_string(clear_resp)).unwrap();
        assert_eq!(deleted.deleted, 1);
        assert!(state.rules.is_empty());
    }

    #[test]
    fn password_from_query_defaults_and_decodes() {
        assert_eq!(password_from_query(None), DEFAULT_TRUSTSTORE_PASSWORD);
        assert_eq!(password_from_query(Some("password=abc")), "abc");
        assert_eq!(password_from_query(Some("other=1&password=a%20b")), "a b");
    }

    #[test]
    fn add_rules_from_bytes_stores_rule_and_rejects_bad_json() {
        let state = test_state();
        let json =
            br#"{"host":"cdn.example.com","action":{"serve":{"statusCode":418,"body":"brew"}}}"#;
        let resp = add_rules_from_bytes(json, &state);
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(state.rules.len(), 1);
        assert_eq!(
            state.rules.list()[0].host.as_deref(),
            Some("cdn.example.com")
        );

        let bad = add_rules_from_bytes(b"{not json", &state);
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.rules.len(), 1, "a rejected body must not add a rule");
    }

    // AC1/AC4: a rule created via the admin handler actually drives interception end-to-end.
    #[tokio::test]
    async fn rule_added_via_admin_handler_is_served_through_listener() {
        use crate::intercept::InterceptListener;
        use rift_core::proxy::intercept_ca::SniCertResolver;

        let ca = CertificateAuthority::generate().expect("ca");
        let ca_pem = ca.ca_cert_pem().to_string();
        let ca = Arc::new(ca);
        let state = InterceptState {
            rules: InterceptRules::new(),
            ca: ca.clone(),
        };

        // Add the rule through the ADMIN handler path (not InterceptRules::add directly).
        let json = br#"{"host":"cdn.example.com","action":{"serve":{"statusCode":418,"body":"admin-brewed"}}}"#;
        assert_eq!(
            add_rules_from_bytes(json, &state).status(),
            StatusCode::CREATED
        );

        let resolver = Arc::new(SniCertResolver::new(ca));
        let listener = InterceptListener::bind(
            "127.0.0.1:0".parse().unwrap(),
            resolver,
            state.rules.clone(),
        )
        .await
        .expect("bind");
        let proxy_url = format!("http://{}", listener.local_addr());
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::https(&proxy_url).unwrap())
            .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).unwrap())
            .build()
            .unwrap();
        let resp = client
            .get("https://cdn.example.com/x")
            .send()
            .await
            .expect("intercepted");
        assert_eq!(resp.status(), 418);
        assert_eq!(resp.text().await.unwrap(), "admin-brewed");

        listener.shutdown().await;
    }

    fn body_bytes(resp: Response<Full<Bytes>>) -> Vec<u8> {
        use http_body_util::BodyExt;
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resp.into_body().collect())
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    fn body_string(resp: Response<Full<Bytes>>) -> String {
        String::from_utf8(body_bytes(resp)).unwrap()
    }
}

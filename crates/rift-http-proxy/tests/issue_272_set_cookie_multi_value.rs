//! Issue #272 gate: multi-value response headers (e.g. multiple `Set-Cookie`) must NOT be
//! comma-folded when a `copy`/`lookup`/`decorate` behavior is present. RFC 7230 §3.2.2 exempts
//! `Set-Cookie` from list folding, so folding two cookie lines into one corrupts cookies.

use rift_http_proxy::imposter::ImposterManager;
use std::time::Duration;

async fn mk(manager: &ImposterManager, cfg: serde_json::Value) {
    let config = serde_json::from_value(cfg).expect("config");
    manager.create_imposter(config).await.expect("create");
    tokio::time::sleep(Duration::from_millis(150)).await;
}

fn set_cookie_count(resp: &reqwest::Response) -> usize {
    resp.headers().get_all("set-cookie").iter().count()
}

/// AC1 — a `copy` behavior must not collapse multiple `Set-Cookie` lines into one.
#[tokio::test]
async fn copy_behavior_preserves_multi_value_set_cookie() {
    let manager = ImposterManager::new();
    mk(
        &manager,
        serde_json::json!({
            "port": 19272, "protocol": "http", "stubs": [
                { "responses": [{
                    "is": { "statusCode": 200,
                        "headers": { "Set-Cookie": ["a=1", "b=2"] },
                        "body": "x=${q}" },
                    "_behaviors": { "copy": { "from": { "query": "q" }, "into": "${q}",
                        "using": { "method": "regex", "selector": ".*" } } } }] }
            ]
        }),
    )
    .await;

    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:19272/x?q=hi")
        .send()
        .await
        .expect("send");
    assert_eq!(
        set_cookie_count(&resp),
        2,
        "two Set-Cookie lines must survive a copy behavior"
    );
    let _ = manager.delete_imposter(19272).await;
}

/// AC2 — a `lookup` behavior must not collapse multiple `Set-Cookie` lines into one.
#[tokio::test]
async fn lookup_behavior_preserves_multi_value_set_cookie() {
    let csv = std::env::temp_dir().join(format!("rift_272_lookup_{}.csv", std::process::id()));
    std::fs::write(&csv, "id,name\nhi,World\n").expect("write csv");

    let manager = ImposterManager::new();
    mk(
        &manager,
        serde_json::json!({
            "port": 19275, "protocol": "http", "stubs": [
                { "responses": [{
                    "is": { "statusCode": 200,
                        "headers": { "Set-Cookie": ["a=1", "n=${row}[name]"] },
                        "body": "n=${row}[name]" },
                    "_behaviors": { "lookup": {
                        "key": { "from": { "query": "q" },
                            "using": { "method": "regex", "selector": ".*" } },
                        "fromDataSource": { "csv": { "path": csv.to_string_lossy(), "keyColumn": "id" } },
                        "into": "${row}" } } }] }
            ]
        }),
    )
    .await;

    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:19275/x?q=hi")
        .send()
        .await
        .expect("send");
    let cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(String::from)
        .collect();
    assert_eq!(
        cookies,
        vec!["a=1".to_string(), "n=World".to_string()],
        "two Set-Cookie lines must survive a lookup behavior, each substituted"
    );
    let _ = manager.delete_imposter(19275).await;
    let _ = std::fs::remove_file(&csv);
}

/// AC3 — the `decorate` path uses a single-value object model but must still exempt `Set-Cookie`,
/// including a lowercase header key, while leaving ordinary single-value headers intact.
#[tokio::test]
async fn decorate_behavior_preserves_multi_value_set_cookie() {
    let manager = ImposterManager::new();
    mk(
        &manager,
        serde_json::json!({
            "port": 19274, "protocol": "http", "stubs": [
                { "responses": [{
                    "is": { "statusCode": 200,
                        "headers": { "set-cookie": ["a=1", "b=2"], "X-Keep": "kept" },
                        "body": "original" },
                    "_behaviors": { "decorate": "config => { config.response.body = 'decorated'; }" } }] }
            ]
        }),
    )
    .await;

    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:19274/x")
        .send()
        .await
        .expect("send");
    let count = set_cookie_count(&resp);
    let keep = resp
        .headers()
        .get("x-keep")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body = resp.text().await.expect("body");
    assert_eq!(
        count, 2,
        "two Set-Cookie lines must survive a decorate behavior (lowercase key)"
    );
    assert_eq!(
        keep.as_deref(),
        Some("kept"),
        "ordinary single-value headers must survive the decorate round-trip"
    );
    assert_eq!(body, "decorated", "decorate must still transform the body");
    let _ = manager.delete_imposter(19274).await;
}

/// AC3 (override) — a decorate script that sets its own Set-Cookie wins; the held-aside originals
/// are not silently re-added on top of it.
#[tokio::test]
async fn decorate_script_set_cookie_overrides_originals() {
    let manager = ImposterManager::new();
    mk(
        &manager,
        serde_json::json!({
            "port": 19276, "protocol": "http", "stubs": [
                { "responses": [{
                    "is": { "statusCode": 200,
                        "headers": { "Set-Cookie": ["a=1", "b=2"] },
                        "body": "original" },
                    "_behaviors": { "decorate": "config => { config.response.headers['Set-Cookie'] = 'z=9'; }" } }] }
            ]
        }),
    )
    .await;

    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:19276/x")
        .send()
        .await
        .expect("send");
    let cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(String::from)
        .collect();
    assert_eq!(
        cookies,
        vec!["z=9".to_string()],
        "a script-set Set-Cookie must win, replacing the held-aside originals"
    );
    let _ = manager.delete_imposter(19276).await;
}

/// AC4 — token substitution still works on single-value headers and the body when behaviors run.
#[tokio::test]
async fn copy_behavior_substitutes_single_value_header_and_body() {
    let manager = ImposterManager::new();
    mk(
        &manager,
        serde_json::json!({
            "port": 19273, "protocol": "http", "stubs": [
                { "responses": [{
                    "is": { "statusCode": 200,
                        "headers": { "X-Token": "v=${q}" },
                        "body": "x=${q}" },
                    "_behaviors": { "copy": { "from": { "query": "q" }, "into": "${q}",
                        "using": { "method": "regex", "selector": ".*" } } } }] }
            ]
        }),
    )
    .await;

    let resp = reqwest::Client::new()
        .get("http://127.0.0.1:19273/x?q=hi")
        .send()
        .await
        .expect("send");
    let token = resp
        .headers()
        .get("x-token")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body = resp.text().await.expect("body");
    assert_eq!(
        token.as_deref(),
        Some("v=hi"),
        "copy substitution must still apply to a single-value header"
    );
    assert_eq!(
        body, "x=hi",
        "copy substitution must still apply to the body"
    );
    let _ = manager.delete_imposter(19273).await;
}

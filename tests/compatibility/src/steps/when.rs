//! When step definitions

use crate::world::CompatibilityWorld;
use cucumber::{gherkin::Step, when};

#[when(expr = "I send GET request to {string} on both services")]
async fn send_get_both(world: &mut CompatibilityWorld, path: String) {
    world.send_to_both("GET", &path, None, None).await
        .expect("Failed to send GET request");
}

#[when(expr = "I send POST request to {string} on both services")]
async fn send_post_both(world: &mut CompatibilityWorld, path: String) {
    world.send_to_both("POST", &path, None, None).await
        .expect("Failed to send POST request");
}

#[when(expr = "I send DELETE request to {string} on both services")]
async fn send_delete_both(world: &mut CompatibilityWorld, path: String) {
    world.send_to_both("DELETE", &path, None, None).await
        .expect("Failed to send DELETE request");
}

#[when(expr = "I send DELETE request to {string} on both admin APIs")]
async fn send_delete_admin(world: &mut CompatibilityWorld, path: String) {
    world.send_to_both("DELETE", &path, None, None).await
        .expect("Failed to send DELETE request");
}

#[when(expr = "I create an imposter on both services:")]
async fn create_imposter(world: &mut CompatibilityWorld, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world.create_imposter_on_both(&config).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[when(expr = "I PUT to {string} on both services:")]
async fn put_to_both(world: &mut CompatibilityWorld, path: String, step: &Step) {
    let body = step.docstring().expect("Missing docstring").to_string();
    // For PUT /imposters, we need to adjust port numbers in the body for Rift
    if path == "/imposters" {
        let rift_body = world.adjust_imposters_body_for_rift(&body).unwrap_or(body.clone());
        let mb_url = format!("{}{}", world.config.mb_admin_url, path);
        let rift_url = format!("{}{}", world.config.rift_admin_url, path);

        let client = world.client.clone();
        let (mb_response, rift_response) = tokio::join!(
            client.put(&mb_url).header("Content-Type", "application/json").body(body.clone()).send(),
            client.put(&rift_url).header("Content-Type", "application/json").body(rift_body).send()
        );
        let mb_resp = mb_response.expect("Failed to PUT on Mountebank");
        let rift_resp = rift_response.expect("Failed to PUT on Rift");

        let mb_status = mb_resp.status().as_u16();
        let rift_status = rift_resp.status().as_u16();
        let mb_body = mb_resp.text().await.unwrap_or_default();
        let rift_body = rift_resp.text().await.unwrap_or_default();

        world.last_response = Some(crate::world::DualResponse {
            mb_status,
            mb_body,
            mb_headers: std::collections::HashMap::new(),
            mb_duration: std::time::Duration::ZERO,
            rift_status,
            rift_body,
            rift_headers: std::collections::HashMap::new(),
            rift_duration: std::time::Duration::ZERO,
        });
    } else {
        world.send_to_both("PUT", &path, Some(&body), None).await
            .expect("Failed to send PUT request");
    }
}

#[when(expr = "I send GET request to {string} on imposter {int}")]
async fn send_get_imposter(world: &mut CompatibilityWorld, path: String, port: u16) {
    // For fault injection tests, connection errors are expected
    // Store the result and let the Then step verify
    let result = world.send_to_imposter(port, "GET", &path, None, None).await;
    if result.is_err() {
        // Mark that a connection error occurred - expected for fault injection
        world.connection_error = true;
    } else {
        world.connection_error = false;
    }
}

#[when(expr = "I send POST request to {string} on imposter {int}")]
async fn send_post_imposter(world: &mut CompatibilityWorld, path: String, port: u16) {
    world.send_to_imposter(port, "POST", &path, None, None).await
        .expect("Failed to send POST request to imposter");
}

#[when(expr = "I send DELETE request to {string} on imposter {int}")]
async fn send_delete_imposter(world: &mut CompatibilityWorld, path: String, port: u16) {
    world.send_to_imposter(port, "DELETE", &path, None, None).await
        .expect("Failed to send DELETE request to imposter");
}

#[when(expr = "I send POST request with body {string} on imposter {int}")]
async fn send_post_with_body(world: &mut CompatibilityWorld, body: String, port: u16) {
    world.send_to_imposter(port, "POST", "/", Some(&body), None).await
        .expect("Failed to send POST request");
}

#[when(expr = "I send POST request to {string} with body {string} on imposter {int}")]
async fn send_post_to_path_with_body(world: &mut CompatibilityWorld, path: String, body: String, port: u16) {
    world.send_to_imposter(port, "POST", &path, Some(&body), None).await
        .expect("Failed to send POST request");
}

#[when(expr = "I send GET request with header {string} on imposter {int}")]
async fn send_get_with_header(world: &mut CompatibilityWorld, header: String, port: u16) {
    let parts: Vec<&str> = header.splitn(2, ": ").collect();
    if parts.len() == 2 {
        let headers = vec![(parts[0].to_string(), parts[1].to_string())];
        world.send_to_imposter(port, "GET", "/", None, Some(&headers)).await
            .expect("Failed to send GET request");
    }
}

#[when(expr = "I send POST request to {string} with header {string} on imposter {int}")]
async fn send_post_with_header(world: &mut CompatibilityWorld, path: String, header: String, port: u16) {
    let parts: Vec<&str> = header.splitn(2, ": ").collect();
    if parts.len() == 2 {
        let headers = vec![(parts[0].to_string(), parts[1].to_string())];
        world.send_to_imposter(port, "POST", &path, None, Some(&headers)).await
            .expect("Failed to send POST request");
    }
}

#[when(expr = "I send GET request with headers on imposter {int}:")]
async fn send_get_with_headers_table(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let mut headers = Vec::new();
    if let Some(table) = step.table() {
        for row in table.rows.iter().skip(1) {
            // Skip header row
            if row.len() >= 2 {
                headers.push((row[0].clone(), row[1].clone()));
            }
        }
    }
    world
        .send_to_imposter(port, "GET", "/", None, Some(&headers))
        .await
        .expect("Failed to send GET request with headers");
}

#[when(expr = "I send {int} GET requests to {string} on imposter {int}")]
async fn send_multiple_get(world: &mut CompatibilityWorld, count: usize, path: String, port: u16) {
    world.clear_response_sequence();
    for _ in 0..count {
        world.send_to_imposter(port, "GET", &path, None, None).await
            .expect("Failed to send GET request");
    }
}

#[when(expr = "I send GET request to {string} on imposter {int} and measure time")]
async fn send_get_measure_time(world: &mut CompatibilityWorld, path: String, port: u16) {
    world.send_to_imposter(port, "GET", &path, None, None).await
        .expect("Failed to send GET request");
}

#[when(expr = "I add a stub to imposter {int} on both services:")]
async fn add_stub(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let stub = step.docstring().expect("Missing docstring").to_string();
    world.add_stub_to_both(port, &stub).await
        .expect("Failed to add stub");
}

#[when(expr = "I add a stub at index {int} to imposter {int} on both services:")]
async fn add_stub_at_index(world: &mut CompatibilityWorld, index: usize, port: u16, step: &Step) {
    let stub = step.docstring().expect("Missing docstring").to_string();
    let wrapped = format!(r#"{{"index": {}, "stub": {}}}"#, index, stub);
    // Both services use the same port numbers (Docker handles the mapping)
    let mb_url = format!("{}/imposters/{}/stubs", world.config.mb_admin_url, port);
    let rift_url = format!("{}/imposters/{}/stubs", world.config.rift_admin_url, port);

    let client = &world.client;
    let (mb_response, rift_response) = tokio::join!(
        client.post(&mb_url).header("Content-Type", "application/json").body(wrapped.clone()).send(),
        client.post(&rift_url).header("Content-Type", "application/json").body(wrapped.clone()).send()
    );
    mb_response.expect("Failed to add stub on Mountebank");
    rift_response.expect("Failed to add stub on Rift");
}

#[when(expr = "I replace stub {int} on imposter {int} on both services:")]
async fn replace_stub(world: &mut CompatibilityWorld, stub_index: usize, port: u16, step: &Step) {
    let stub = step.docstring().expect("Missing docstring").to_string();
    world.replace_stub_on_both(port, stub_index, &stub).await
        .expect("Failed to replace stub");
}

#[when(expr = "I send {int} concurrent GET requests to {string} on imposter {int}")]
async fn send_concurrent_requests(world: &mut CompatibilityWorld, count: usize, path: String, port: u16) {
    let mut handles = Vec::new();

    for _ in 0..count {
        let mb_url = format!("{}{}", world.get_imposter_url(port, crate::world::Service::Mountebank), path);
        let rift_url = format!("{}{}", world.get_imposter_url(port, crate::world::Service::Rift), path);
        let client = world.client.clone();

        handles.push(tokio::spawn(async move {
            let _ = client.get(&mb_url).send().await;
            let _ = client.get(&rift_url).send().await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

#[when(expr = "I send POST request with {int}KB body on imposter {int}")]
async fn send_large_body(world: &mut CompatibilityWorld, size_kb: usize, port: u16) {
    let body = "x".repeat(size_kb * 1024);
    world.send_to_imposter(port, "POST", "/", Some(&body), None).await
        .expect("Failed to send large body");
}

#[when(regex = r#"^I send POST request with JSON body '(.+)' on imposter (\d+)$"#)]
async fn send_post_json_body(world: &mut CompatibilityWorld, body: String, port: String) {
    let port: u16 = port.parse().unwrap();
    let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    world.send_to_imposter(port, "POST", "/", Some(&body), Some(&headers)).await
        .expect("Failed to send POST request with JSON body");
}

#[when(regex = r#"^I send POST request with header "([^"]+)" on imposter (\d+)$"#)]
async fn send_post_with_header_regex(world: &mut CompatibilityWorld, header: String, port: String) {
    let port: u16 = port.parse().unwrap();
    let mut headers = Vec::new();
    if let Some((key, value)) = header.split_once(": ") {
        headers.push((key.to_string(), value.to_string()));
    }
    world.send_to_imposter(port, "POST", "/", None, Some(&headers)).await
        .expect("Failed to send POST request with header");
}

#[when(regex = r#"^I send GET request with header "([^"]+): ([^"]+)" on imposter (\d+) and measure time$"#)]
async fn send_get_with_header_measure(world: &mut CompatibilityWorld, key: String, value: String, port: String) {
    let port: u16 = port.parse().unwrap();
    let headers = vec![(key, value)];
    world.send_to_imposter(port, "GET", "/", None, Some(&headers)).await
        .expect("Failed to send GET request with header");
}

// Note: Table parsing for headers is handled separately
// This step can be implemented when needed for specific scenarios

#[when(expr = "I try to add an invalid stub to imposter {int}")]
async fn try_add_invalid_stub(world: &mut CompatibilityWorld, port: u16) {
    let invalid = r#"{"invalid": "not a valid stub"}"#;
    // Ignore errors - we expect this to fail
    let _ = world.add_stub_to_both(port, invalid).await;
}

#[when(expr = "I try to create another imposter on port {int} on both services")]
async fn try_create_duplicate(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(r#"{{"port": {}, "protocol": "http"}}"#, port);
    let _ = world.create_imposter_on_both(&imposter).await;
}

#[when(expr = "I export imposters with replayable=true from both services")]
async fn export_imposters(world: &mut CompatibilityWorld) {
    world.send_to_both("GET", "/imposters?replayable=true", None, None).await
        .expect("Failed to export imposters");
}

#[when(expr = "I delete all imposters on both services")]
async fn delete_all_imposters(world: &mut CompatibilityWorld) {
    world.clear_imposters().await.expect("Failed to delete imposters");
}

#[when(expr = "I reimport the exported configuration")]
async fn reimport_config(world: &mut CompatibilityWorld) {
    // Use the last response to reimport - clone to avoid borrow issue
    let body = world.last_response.as_ref().map(|r| r.mb_body.clone());
    if let Some(ref body) = body {
        let _ = world.send_to_both("PUT", "/imposters", Some(body), None).await;
    }
}

#[when(expr = "I send POST with invalid JSON to {string} on both services")]
async fn send_invalid_json(world: &mut CompatibilityWorld, path: String) {
    let invalid_json = "{ this is not valid json }";
    let response = world.send_to_both("POST", &path, Some(invalid_json), None).await;
    if let Ok(r) = response {
        world.last_response = Some(r);
    }
}

#[when(expr = "I POST to {string} with missing required fields on both services:")]
async fn post_missing_fields(world: &mut CompatibilityWorld, path: String, step: &cucumber::gherkin::Step) {
    let body = step.docstring().expect("Missing docstring").to_string();
    let response = world.send_to_both("POST", &path, Some(&body), None).await;
    if let Ok(r) = response {
        world.last_response = Some(r);
    }
}

#[when(expr = "I send GET request with header {string}: {string} on imposter {int}")]
async fn send_get_with_header_value(world: &mut CompatibilityWorld, key: String, value: String, port: u16) {
    let headers = vec![(key, value)];
    world.send_to_imposter(port, "GET", "/", None, Some(&headers)).await
        .expect("Failed to send GET request with header");
}

// ============================================
// Form data step definitions
// ============================================

#[when(expr = "I send POST request with form body {string} and Content-Type {string} on imposter {int}")]
async fn send_post_form_body(
    world: &mut CompatibilityWorld,
    body: String,
    content_type: String,
    port: u16,
) {
    let headers = vec![("Content-Type".to_string(), content_type)];
    world
        .send_to_imposter(port, "POST", "/", Some(&body), Some(&headers))
        .await
        .expect("Failed to send POST request with form body");
}

// ============================================
// Rift-only step definitions
// ============================================

#[when(expr = "I create an imposter on Rift only:")]
async fn create_imposter_rift_only(world: &mut CompatibilityWorld, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world
        .send_to_rift("POST", "/imposters", Some(&config))
        .await
        .expect("Failed to create imposter on Rift");
}

// ============================================
// Mountebank-only step definitions
// ============================================

#[when(expr = "I POST to {string} with missing required fields on Mountebank:")]
async fn post_missing_fields_mb_only(
    world: &mut CompatibilityWorld,
    path: String,
    step: &cucumber::gherkin::Step,
) {
    let body = step.docstring().expect("Missing docstring").to_string();
    world
        .send_to_mountebank("POST", &path, Some(&body))
        .await
        .expect("Failed to send POST to Mountebank");
}

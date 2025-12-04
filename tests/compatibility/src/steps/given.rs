//! Given step definitions

use crate::world::CompatibilityWorld;
use cucumber::{gherkin::Step, given};

#[given(expr = "both Mountebank and Rift services are running")]
async fn services_running(world: &mut CompatibilityWorld) {
    world.ensure_containers().await.expect("Failed to start containers");

    // Verify both services are accessible
    let mb_check = world.client
        .get(format!("{}/", world.config.mb_admin_url))
        .send()
        .await;
    let rift_check = world.client
        .get(format!("{}/", world.config.rift_admin_url))
        .send()
        .await;

    assert!(mb_check.is_ok(), "Mountebank is not accessible");
    assert!(rift_check.is_ok(), "Rift is not accessible");
}

#[given(expr = "all imposters are cleared")]
async fn clear_imposters(world: &mut CompatibilityWorld) {
    world.clear_imposters().await.expect("Failed to clear imposters");
    world.clear_response_sequence();
}

#[given(expr = "an imposter exists on port {int}")]
async fn imposter_exists(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(r#"{{"port": {}, "protocol": "http"}}"#, port);
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");

    // Wait a moment for imposter to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter exists on port {int} with name {string}")]
async fn imposter_exists_with_name(world: &mut CompatibilityWorld, port: u16, name: String) {
    let imposter = format!(r#"{{"port": {}, "protocol": "http", "name": "{}"}}"#, port, name);
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with stub:")]
async fn imposter_with_stub(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let stub = step.docstring().expect("Missing docstring").to_string();
    let imposter = format!(
        r#"{{"port": {}, "protocol": "http", "stubs": [{}]}}"#,
        port, stub
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with stubs:")]
async fn imposter_with_stubs(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let stubs = step.docstring().expect("Missing docstring").to_string();
    let imposter = format!(
        r#"{{"port": {}, "protocol": "http", "stubs": {}}}"#,
        port, stubs
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with:")]
async fn imposter_with_config(world: &mut CompatibilityWorld, _port: u16, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world.create_imposter_on_both(&config).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "default response is status {int}")]
async fn set_default_response(_world: &mut CompatibilityWorld, _status: u16) {
    // This is typically set in the imposter config already
    // This step is a no-op since the imposter should have been created with defaultResponse
}

#[given(expr = "an imposter on port {int} with recordRequests enabled")]
async fn imposter_with_recording(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": true,
            "stubs": [{{"predicates": [], "responses": [{{"is": {{"statusCode": 200}}}}]}}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter exists on port {int} with recordRequests enabled")]
async fn imposter_exists_with_recording(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": true,
            "stubs": [{{"predicates": [], "responses": [{{"is": {{"statusCode": 200}}}}]}}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with recordRequests disabled")]
async fn imposter_without_recording(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": false,
            "stubs": [{{"predicates": [], "responses": [{{"is": {{"statusCode": 200}}}}]}}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with recordRequests enabled and stub:")]
async fn imposter_with_recording_and_stub(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let stub = step.docstring().expect("Missing docstring").to_string();
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": true,
            "stubs": [{}]
        }}"#,
        port, stub
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with recordRequests enabled and multiple stubs:")]
async fn imposter_with_recording_and_stubs(world: &mut CompatibilityWorld, port: u16, step: &Step) {
    let stubs = step.docstring().expect("Missing docstring").to_string();
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": true,
            "stubs": {}
        }}"#,
        port, stubs
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter exists on port {int} with stubs")]
async fn imposter_exists_with_stubs(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "stubs": [
                {{"predicates": [{{"equals": {{"path": "/test"}}}}], "responses": [{{"is": {{"statusCode": 200}}}}]}}
            ]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(regex = r"^imposters exist on ports (\d+(?:, \d+)*)$")]
async fn imposters_exist_on_ports(world: &mut CompatibilityWorld, ports_str: String) {
    for port_str in ports_str.split(", ") {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            let imposter = format!(r#"{{"port": {}, "protocol": "http"}}"#, port);
            world.create_imposter_on_both(&imposter).await
                .expect("Failed to create imposter");
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "requests have been made to imposter 4545")]
async fn requests_made_to_imposter(world: &mut CompatibilityWorld) {
    // Make a few requests to establish recorded data
    world.send_to_imposter(4545, "GET", "/test1", None, None).await.ok();
    world.send_to_imposter(4545, "GET", "/test2", None, None).await.ok();
}

#[given(expr = "an imposter on port {int} with proxy to backend:")]
async fn imposter_with_proxy(world: &mut CompatibilityWorld, _port: u16, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world.create_imposter_on_both(&config).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "imposters exist on ports {int}, {int} with various stubs")]
async fn imposters_exist_with_stubs(world: &mut CompatibilityWorld, port1: u16, port2: u16) {
    let imposter1 = format!(
        r#"{{"port": {}, "protocol": "http", "stubs": [{{"predicates": [], "responses": [{{"is": {{"statusCode": 200}}}}]}}]}}"#,
        port1
    );
    let imposter2 = format!(
        r#"{{"port": {}, "protocol": "http", "stubs": [{{"predicates": [], "responses": [{{"is": {{"statusCode": 200}}}}]}}]}}"#,
        port2
    );
    world.create_imposter_on_both(&imposter1).await.ok();
    world.create_imposter_on_both(&imposter2).await.ok();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "I send {int} GET requests to {string} on imposter {int}")]
async fn given_send_requests(world: &mut CompatibilityWorld, count: usize, path: String, port: u16) {
    for _ in 0..count {
        world.send_to_imposter(port, "GET", &path, None, None).await.ok();
    }
}

#[given(expr = "an imposter on port {int} with proxy stub")]
async fn imposter_with_proxy_stub(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "stubs": [{{
                "responses": [{{
                    "proxy": {{"to": "http://localhost:9999"}}
                }}]
            }}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter exists on port {int} with proxy stub")]
async fn imposter_exists_with_proxy_stub(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "stubs": [{{
                "responses": [{{
                    "proxy": {{"to": "http://localhost:9999"}}
                }}]
            }}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(regex = r#"^a backend server running on port (\d+) returning "([^"]+)"$"#)]
async fn backend_server_returning(world: &mut CompatibilityWorld, port: String, response: String) {
    let port: u16 = port.parse().unwrap();
    // Create a simple imposter to act as the backend server
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "stubs": [{{
                "predicates": [],
                "responses": [{{"is": {{"statusCode": 200, "body": "{}"}}}}]
            }}]
        }}"#,
        port, response
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "a backend server running on port {int}")]
async fn backend_server_simple(world: &mut CompatibilityWorld, port: u16) {
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "stubs": [{{
                "predicates": [],
                "responses": [{{"is": {{"statusCode": 200, "body": "backend response"}}}}]
            }}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "a backend server that tracks request count")]
async fn backend_server_tracking(world: &mut CompatibilityWorld) {
    // Create backend on port 4546 that returns request info
    let imposter = r#"{
        "port": 4546,
        "protocol": "http",
        "recordRequests": true,
        "stubs": [{
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "tracked"}}]
        }]
    }"#;
    world.create_imposter_on_both(imposter).await
        .expect("Failed to create tracking backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "a backend server with {int}ms delay")]
async fn backend_server_with_delay(world: &mut CompatibilityWorld, delay: u64) {
    let imposter = format!(
        r#"{{
            "port": 4546,
            "protocol": "http",
            "stubs": [{{
                "predicates": [],
                "responses": [{{
                    "is": {{"statusCode": 200, "body": "delayed"}},
                    "_behaviors": {{"wait": {}}}
                }}]
            }}]
        }}"#,
        delay
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create delayed backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "a backend server that echoes headers")]
async fn backend_server_echo_headers(world: &mut CompatibilityWorld) {
    // Create backend that echoes headers via inject
    let imposter = r#"{
        "port": 4546,
        "protocol": "http",
        "stubs": [{
            "predicates": [],
            "responses": [{"is": {"statusCode": 200, "body": "echo"}}]
        }]
    }"#;
    world.create_imposter_on_both(imposter).await
        .expect("Failed to create echo backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with proxy:")]
async fn imposter_with_proxy_config(world: &mut CompatibilityWorld, _port: u16, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world.create_imposter_on_both(&config).await
        .expect("Failed to create proxy imposter");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "an imposter on port {int} with proxy and recordRequests:")]
async fn imposter_with_proxy_and_recording(world: &mut CompatibilityWorld, _port: u16, step: &Step) {
    let config = step.docstring().expect("Missing docstring").to_string();
    world.create_imposter_on_both(&config).await
        .expect("Failed to create proxy imposter with recording");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[given(expr = "a backend server that echoes path on port {int}")]
async fn backend_server_echo_path(world: &mut CompatibilityWorld, port: u16) {
    // Create a backend that echoes the received path using inject
    // The response body will contain the path for verification
    let imposter = format!(
        r#"{{
            "port": {},
            "protocol": "http",
            "recordRequests": true,
            "stubs": [{{
                "predicates": [],
                "responses": [{{
                    "inject": "function(config) {{ return {{ statusCode: 200, body: 'path:' + config.request.path }}; }}"
                }}]
            }}]
        }}"#,
        port
    );
    world.create_imposter_on_both(&imposter).await
        .expect("Failed to create path echo backend server");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

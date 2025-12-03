//! Rift Stub Verifier CLI Tool
//!
//! This tool fetches imposter configurations and verifies that stubs respond
//! as expected by simulating API calls based on the predicate definitions.
//!
//! Usage:
//!   rift-verify --admin-url http://localhost:2525 [OPTIONS]
//!
//! Features:
//! - Fetches all imposters from the admin API
//! - Generates test requests based on stub predicates
//! - Verifies responses match expected values
//! - Optionally generates curl commands
//! - Provides detailed failure reports

// Allow unused fields that may be used in future versions or for debugging
#![allow(dead_code)]

use clap::Parser;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

// ANSI color codes
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Rift Stub Verifier - Test your imposters and stubs
#[derive(Parser, Debug)]
#[command(name = "rift-verify")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Rift admin API URL
    #[arg(short, long, default_value = "http://localhost:2525")]
    admin_url: String,

    /// Specific imposter port to verify (optional, verifies all if not specified)
    #[arg(short, long)]
    port: Option<u16>,

    /// Show curl commands for each test
    #[arg(short = 'c', long)]
    show_curl: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Request timeout in seconds
    #[arg(short, long, default_value = "10")]
    timeout: u64,

    /// Only run dry-run (don't make actual requests, just show what would be tested)
    #[arg(long)]
    dry_run: bool,

    /// Skip stubs with inject/proxy/script responses (can't verify dynamically generated responses)
    #[arg(long, default_value = "true")]
    skip_dynamic: bool,
}

// ============================================================================
// API Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
struct RootResponse {
    #[serde(default)]
    imposters: Option<Vec<ImposterLink>>,
}

#[derive(Debug, Deserialize)]
struct ImposterLink {
    port: u16,
    protocol: String,
    #[serde(rename = "_links")]
    links: Option<HashMap<String, LinkInfo>>,
}

#[derive(Debug, Deserialize)]
struct LinkInfo {
    href: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImposterDetails {
    port: u16,
    protocol: String,
    name: Option<String>,
    #[serde(default)]
    stubs: Vec<Stub>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stub {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    predicates: Vec<serde_json::Value>,
    #[serde(default)]
    responses: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct TestCase {
    stub_index: usize,
    stub_id: Option<String>,
    method: String,
    path: String,
    headers: HashMap<String, String>,
    query_params: HashMap<String, String>,
    body: Option<String>,
    expected_status: u16,
    expected_headers: HashMap<String, String>,
    expected_body: Option<serde_json::Value>,
    is_dynamic: bool,
    skip_reason: Option<String>,
}

#[derive(Debug)]
struct TestResult {
    test_case: TestCase,
    success: bool,
    actual_status: Option<u16>,
    actual_headers: Option<HashMap<String, String>>,
    actual_body: Option<String>,
    error: Option<String>,
    duration_ms: u128,
}

#[derive(Debug, Default)]
struct VerificationSummary {
    total_imposters: usize,
    total_stubs: usize,
    total_tests: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureDetails>,
}

#[derive(Debug)]
struct FailureDetails {
    imposter_port: u16,
    imposter_name: Option<String>,
    stub_index: usize,
    stub_id: Option<String>,
    test_description: String,
    expected: String,
    actual: String,
    curl_command: Option<String>,
}

// ============================================================================
// Main Logic
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()?;

    println!("{BOLD}{CYAN}Rift Stub Verifier{RESET}");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Admin URL: {}", args.admin_url);
    println!();

    // Fetch imposters
    let imposters = fetch_imposters(&client, &args.admin_url, args.port).await?;

    if imposters.is_empty() {
        println!("{YELLOW}Warning:{RESET} No imposters found");
        return Ok(());
    }

    let mut summary = VerificationSummary {
        total_imposters: imposters.len(),
        ..Default::default()
    };

    // Process each imposter
    for imposter in &imposters {
        println!(
            "{}Imposter:{} {} (port {})",
            BOLD,
            RESET,
            imposter.name.as_deref().unwrap_or("unnamed"),
            imposter.port
        );

        summary.total_stubs += imposter.stubs.len();

        if imposter.stubs.is_empty() {
            println!("   └─ No stubs defined");
            println!();
            continue;
        }

        for (stub_index, stub) in imposter.stubs.iter().enumerate() {
            let test_cases = generate_test_cases(stub_index, stub, args.skip_dynamic);
            summary.total_tests += test_cases.len();

            for test_case in test_cases {
                if args.show_curl || args.verbose {
                    let curl = generate_curl_command(imposter.port, &test_case);
                    println!("   {DIM}{curl}{RESET}");
                }

                if test_case.skip_reason.is_some() {
                    summary.skipped += 1;
                    if args.verbose {
                        println!(
                            "   {}SKIP{} Stub #{} - {}",
                            YELLOW,
                            RESET,
                            stub_index,
                            test_case.skip_reason.as_ref().unwrap()
                        );
                    }
                    continue;
                }

                if args.dry_run {
                    println!(
                        "   {}DRY-RUN{} Stub #{}{} - {} {}",
                        CYAN,
                        RESET,
                        stub_index,
                        test_case
                            .stub_id
                            .as_ref()
                            .map(|id| format!(" [{id}]"))
                            .unwrap_or_default(),
                        test_case.method,
                        test_case.path
                    );
                    summary.skipped += 1;
                    continue;
                }

                let result = execute_test(&client, imposter.port, &test_case).await;

                if result.success {
                    summary.passed += 1;
                    if args.verbose {
                        println!(
                            "   {}PASS{} Stub #{}{} - {} {} -> {} ({}ms)",
                            GREEN,
                            RESET,
                            stub_index,
                            test_case
                                .stub_id
                                .as_ref()
                                .map(|id| format!(" [{id}]"))
                                .unwrap_or_default(),
                            test_case.method,
                            test_case.path,
                            result.actual_status.unwrap_or(0),
                            result.duration_ms
                        );
                    }
                } else {
                    summary.failed += 1;
                    let failure = FailureDetails {
                        imposter_port: imposter.port,
                        imposter_name: imposter.name.clone(),
                        stub_index,
                        stub_id: test_case.stub_id.clone(),
                        test_description: format!("{} {}", test_case.method, test_case.path),
                        expected: format!(
                            "status={}, body={:?}",
                            test_case.expected_status, test_case.expected_body
                        ),
                        actual: if let Some(err) = &result.error {
                            format!("error: {err}")
                        } else {
                            format!(
                                "status={}, body={:?}",
                                result.actual_status.unwrap_or(0),
                                result.actual_body
                            )
                        },
                        curl_command: Some(generate_curl_command(imposter.port, &test_case)),
                    };

                    println!(
                        "   {}FAIL{} Stub #{}{} - {} {}",
                        RED,
                        RESET,
                        stub_index,
                        test_case
                            .stub_id
                            .as_ref()
                            .map(|id| format!(" [{id}]"))
                            .unwrap_or_default(),
                        test_case.method,
                        test_case.path
                    );

                    summary.failures.push(failure);
                }
            }
        }
        println!();
    }

    // Print summary
    print_summary(&summary, args.show_curl);

    // Exit with error code if any failures
    if summary.failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

// ============================================================================
// Imposter Fetching
// ============================================================================

async fn fetch_imposters(
    client: &Client,
    admin_url: &str,
    filter_port: Option<u16>,
) -> Result<Vec<ImposterDetails>, Box<dyn std::error::Error>> {
    // Get list of imposters
    let imposters_url = format!("{admin_url}/imposters");
    let response = client.get(&imposters_url).send().await?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch imposters: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }

    let imposters_response: serde_json::Value = response.json().await?;

    // Handle both formats: { imposters: [...] } and { imposters: [...], ... }
    let imposter_links: Vec<ImposterLink> =
        if let Some(imposters) = imposters_response.get("imposters") {
            serde_json::from_value(imposters.clone())?
        } else {
            vec![]
        };

    let mut imposters = Vec::new();

    for link in imposter_links {
        if let Some(port) = filter_port {
            if link.port != port {
                continue;
            }
        }

        // Fetch full imposter details
        let detail_url = format!("{}/imposters/{}", admin_url, link.port);
        let detail_response = client.get(&detail_url).send().await?;

        if detail_response.status().is_success() {
            let details: ImposterDetails = detail_response.json().await?;
            imposters.push(details);
        }
    }

    Ok(imposters)
}

// ============================================================================
// Test Case Generation
// ============================================================================

fn generate_test_cases(stub_index: usize, stub: &Stub, skip_dynamic: bool) -> Vec<TestCase> {
    let mut test_cases = Vec::new();

    // Check if this stub has dynamic responses
    let (is_dynamic, skip_reason) = check_if_dynamic(&stub.responses);

    if is_dynamic && skip_dynamic {
        test_cases.push(TestCase {
            stub_index,
            stub_id: stub.id.clone(),
            method: "GET".to_string(),
            path: "/".to_string(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            body: None,
            expected_status: 200,
            expected_headers: HashMap::new(),
            expected_body: None,
            is_dynamic: true,
            skip_reason,
        });
        return test_cases;
    }

    // Extract expected response from first response
    let (expected_status, expected_headers, expected_body) =
        extract_expected_response(&stub.responses);

    // Parse predicates to build test request
    let (method, path, headers, query_params, body) = parse_predicates(&stub.predicates);

    test_cases.push(TestCase {
        stub_index,
        stub_id: stub.id.clone(),
        method,
        path,
        headers,
        query_params,
        body,
        expected_status,
        expected_headers,
        expected_body,
        is_dynamic,
        skip_reason: None,
    });

    test_cases
}

fn check_if_dynamic(responses: &[serde_json::Value]) -> (bool, Option<String>) {
    if responses.is_empty() {
        return (false, None);
    }

    // Multiple responses = cycling behavior (stateful, can't predict which response)
    if responses.len() > 1 {
        return (
            true,
            Some(format!("cycling responses ({} responses)", responses.len())),
        );
    }

    let first = &responses[0];

    if first.get("inject").is_some() {
        return (true, Some("inject response (JavaScript)".to_string()));
    }

    if first.get("proxy").is_some() {
        return (true, Some("proxy response".to_string()));
    }

    if first.get("fault").is_some() {
        return (true, Some("fault injection".to_string()));
    }

    // Check for _rift script extension
    if let Some(rift) = first.get("_rift") {
        if rift.get("script").is_some() {
            return (true, Some("Rift script response".to_string()));
        }
    }

    // Check for _behaviors with repeat (stateful)
    if let Some(behaviors) = first.get("_behaviors") {
        if behaviors.get("repeat").is_some() {
            return (true, Some("repeat behavior (stateful)".to_string()));
        }
    }

    (false, None)
}

fn extract_expected_response(
    responses: &[serde_json::Value],
) -> (u16, HashMap<String, String>, Option<serde_json::Value>) {
    if responses.is_empty() {
        return (200, HashMap::new(), None);
    }

    let first = &responses[0];

    // Handle "is" response format
    if let Some(is_response) = first.get("is") {
        let status = is_response
            .get("statusCode")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;

        let headers = is_response
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let body = is_response.get("body").cloned();

        return (status, headers, body);
    }

    // Direct format without "is" wrapper
    let status = first
        .get("statusCode")
        .and_then(|v| v.as_u64())
        .unwrap_or(200) as u16;

    let headers = first
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let body = first.get("body").cloned();

    (status, headers, body)
}

#[allow(clippy::type_complexity)]
fn parse_predicates(
    predicates: &[serde_json::Value],
) -> (
    String,
    String,
    HashMap<String, String>,
    HashMap<String, String>,
    Option<String>,
) {
    let mut method = "GET".to_string();
    let mut path = "/".to_string();
    let mut headers = HashMap::new();
    let mut query_params = HashMap::new();
    let mut body = None;

    for predicate in predicates {
        // Handle various predicate formats

        // "equals" predicate
        if let Some(equals) = predicate.get("equals") {
            parse_equals_predicate(
                equals,
                &mut method,
                &mut path,
                &mut headers,
                &mut query_params,
                &mut body,
            );
        }

        // "contains" predicate
        if let Some(contains) = predicate.get("contains") {
            parse_contains_predicate(contains, &mut path, &mut headers, &mut body);
        }

        // "startsWith" predicate
        if let Some(starts_with) = predicate.get("startsWith") {
            if let Some(p) = starts_with.get("path").and_then(|v| v.as_str()) {
                path = p.to_string();
            }
        }

        // "matches" predicate (regex - use a sample value)
        if let Some(matches) = predicate.get("matches") {
            if let Some(p) = matches.get("path").and_then(|v| v.as_str()) {
                // Generate a sample path that might match the regex
                path = generate_sample_from_regex(p);
            }
            if let Some(m) = matches.get("method").and_then(|v| v.as_str()) {
                method = generate_sample_from_regex(m);
            }
        }

        // "exists" predicate
        if let Some(exists) = predicate.get("exists") {
            if let Some(hdrs) = exists.get("headers").and_then(|v| v.as_object()) {
                for (name, should_exist) in hdrs {
                    if should_exist.as_bool().unwrap_or(true) {
                        headers.insert(name.clone(), "test-value".to_string());
                    }
                }
            }
        }

        // "deepEquals" predicate
        if let Some(deep_equals) = predicate.get("deepEquals") {
            parse_equals_predicate(
                deep_equals,
                &mut method,
                &mut path,
                &mut headers,
                &mut query_params,
                &mut body,
            );
        }

        // "and" predicate - recursively parse all inner predicates
        if let Some(and_predicates) = predicate.get("and").and_then(|v| v.as_array()) {
            let inner: Vec<serde_json::Value> = and_predicates.clone();
            let (m, p, h, q, b) = parse_predicates(&inner);
            if m != "GET" {
                method = m;
            }
            if p != "/" {
                path = p;
            }
            headers.extend(h);
            query_params.extend(q);
            if b.is_some() {
                body = b;
            }
        }

        // "or" predicate - use first inner predicate
        if let Some(or_predicates) = predicate.get("or").and_then(|v| v.as_array()) {
            if let Some(first) = or_predicates.first() {
                let inner = vec![first.clone()];
                let (m, p, h, q, b) = parse_predicates(&inner);
                if m != "GET" {
                    method = m;
                }
                if p != "/" {
                    path = p;
                }
                headers.extend(h);
                query_params.extend(q);
                if b.is_some() {
                    body = b;
                }
            }
        }
    }

    (method, path, headers, query_params, body)
}

fn parse_equals_predicate(
    equals: &serde_json::Value,
    method: &mut String,
    path: &mut String,
    headers: &mut HashMap<String, String>,
    query_params: &mut HashMap<String, String>,
    body: &mut Option<String>,
) {
    if let Some(m) = equals.get("method").and_then(|v| v.as_str()) {
        *method = m.to_string();
    }

    if let Some(p) = equals.get("path").and_then(|v| v.as_str()) {
        *path = p.to_string();
    }

    if let Some(hdrs) = equals.get("headers").and_then(|v| v.as_object()) {
        for (name, value) in hdrs {
            if let Some(v) = value.as_str() {
                headers.insert(name.clone(), v.to_string());
            }
        }
    }

    if let Some(query) = equals.get("query").and_then(|v| v.as_object()) {
        for (name, value) in query {
            if let Some(v) = value.as_str() {
                query_params.insert(name.clone(), v.to_string());
            }
        }
    }

    if let Some(b) = equals.get("body") {
        if let Some(s) = b.as_str() {
            *body = Some(s.to_string());
        } else {
            *body = Some(serde_json::to_string(b).unwrap_or_default());
        }
    }
}

fn parse_contains_predicate(
    contains: &serde_json::Value,
    path: &mut String,
    headers: &mut HashMap<String, String>,
    body: &mut Option<String>,
) {
    // For "contains", we need to include the substring in our test value
    if let Some(p) = contains.get("path").and_then(|v| v.as_str()) {
        *path = format!("/test{p}");
    }

    if let Some(hdrs) = contains.get("headers").and_then(|v| v.as_object()) {
        for (name, value) in hdrs {
            if let Some(v) = value.as_str() {
                headers.insert(name.clone(), format!("prefix{v}suffix"));
            }
        }
    }

    if let Some(b) = contains.get("body").and_then(|v| v.as_str()) {
        *body = Some(format!("test {b} content"));
    }
}

fn generate_sample_from_regex(pattern: &str) -> String {
    // Simple heuristic to generate a sample that might match common patterns
    // This is a best-effort approach for common regex patterns

    // /api/v\d+/users -> /api/v1/users
    let sample = pattern
        .replace(r"\d+", "1")
        .replace(r"\d", "1")
        .replace(r"\w+", "test")
        .replace(r"\w", "a")
        .replace(r".*", "")
        .replace(r".+", "x")
        .replace("^", "")
        .replace("$", "")
        .replace(r"[^/]+", "item")
        .replace(r"[a-zA-Z]+", "test")
        .replace(r"[0-9]+", "123");

    if sample.is_empty() {
        "/".to_string()
    } else {
        sample
    }
}

// ============================================================================
// Test Execution
// ============================================================================

async fn execute_test(client: &Client, imposter_port: u16, test_case: &TestCase) -> TestResult {
    let start = std::time::Instant::now();

    // Build URL with query params
    let mut url = format!("http://localhost:{}{}", imposter_port, test_case.path);
    if !test_case.query_params.is_empty() {
        let query_string: Vec<String> = test_case
            .query_params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect();
        url = format!("{}?{}", url, query_string.join("&"));
    }

    // Build request
    let mut request = match test_case.method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        _ => client.get(&url),
    };

    // Add headers
    for (name, value) in &test_case.headers {
        request = request.header(name, value);
    }

    // Add body if present
    if let Some(ref body) = test_case.body {
        request = request.body(body.clone());
    }

    // Execute request
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let headers: HashMap<String, String> = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|v| (name.as_str().to_string(), v.to_string()))
                })
                .collect();
            let body_text = response.text().await.ok();

            let duration_ms = start.elapsed().as_millis();

            // Verify response
            let success = verify_response(
                test_case.expected_status,
                &test_case.expected_headers,
                &test_case.expected_body,
                status,
                &headers,
                &body_text,
            );

            TestResult {
                test_case: test_case.clone(),
                success,
                actual_status: Some(status),
                actual_headers: Some(headers),
                actual_body: body_text,
                error: None,
                duration_ms,
            }
        }
        Err(e) => TestResult {
            test_case: test_case.clone(),
            success: false,
            actual_status: None,
            actual_headers: None,
            actual_body: None,
            error: Some(e.to_string()),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

fn verify_response(
    expected_status: u16,
    expected_headers: &HashMap<String, String>,
    expected_body: &Option<serde_json::Value>,
    actual_status: u16,
    actual_headers: &HashMap<String, String>,
    actual_body: &Option<String>,
) -> bool {
    // Check status code
    if expected_status != actual_status {
        return false;
    }

    // Check expected headers (actual may have more headers, that's ok)
    for (name, expected_value) in expected_headers {
        let name_lower = name.to_lowercase();
        let actual_value = actual_headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v);

        if actual_value != Some(expected_value) {
            return false;
        }
    }

    // Check body if expected
    if let Some(expected) = expected_body {
        let actual = match actual_body {
            Some(text) => text,
            None => return false,
        };

        // Try JSON comparison first
        if let Ok(actual_json) = serde_json::from_str::<serde_json::Value>(actual) {
            // For JSON, do deep comparison
            if !json_matches(expected, &actual_json) {
                return false;
            }
        } else {
            // For string comparison
            let expected_str = match expected {
                serde_json::Value::String(s) => s.as_str(),
                _ => return false,
            };
            if actual != expected_str {
                return false;
            }
        }
    }

    true
}

fn json_matches(expected: &serde_json::Value, actual: &serde_json::Value) -> bool {
    match (expected, actual) {
        (serde_json::Value::Object(exp_obj), serde_json::Value::Object(act_obj)) => {
            // All expected keys must be present with matching values
            exp_obj.iter().all(|(key, exp_val)| {
                act_obj
                    .get(key)
                    .map(|act_val| json_matches(exp_val, act_val))
                    .unwrap_or(false)
            })
        }
        (serde_json::Value::Array(exp_arr), serde_json::Value::Array(act_arr)) => {
            exp_arr.len() == act_arr.len()
                && exp_arr
                    .iter()
                    .zip(act_arr.iter())
                    .all(|(e, a)| json_matches(e, a))
        }
        _ => expected == actual,
    }
}

// ============================================================================
// Curl Command Generation
// ============================================================================

fn generate_curl_command(port: u16, test_case: &TestCase) -> String {
    let mut cmd = format!("curl -X {} ", test_case.method);

    // Add headers
    for (name, value) in &test_case.headers {
        cmd.push_str(&format!("-H '{name}: {value}' "));
    }

    // Add body
    if let Some(ref body) = test_case.body {
        let escaped = body.replace('\'', "'\\''");
        cmd.push_str(&format!("-d '{escaped}' "));
    }

    // Build URL with query params
    let mut url = format!("'http://localhost:{}{}", port, test_case.path);
    if !test_case.query_params.is_empty() {
        let query_string: Vec<String> = test_case
            .query_params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        url = format!("{}?{}", url, query_string.join("&"));
    }
    url.push('\'');

    cmd.push_str(&url);
    cmd
}

// ============================================================================
// Summary Report
// ============================================================================

fn print_summary(summary: &VerificationSummary, show_curl: bool) {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("{BOLD}Verification Summary{RESET}");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Imposters:  {}", summary.total_imposters);
    println!("  Stubs:      {}", summary.total_stubs);
    println!("  Tests:      {}", summary.total_tests);
    println!();
    println!("  {}Passed:  {}{}", GREEN, summary.passed, RESET);
    println!("  {}Failed:  {}{}", RED, summary.failed, RESET);
    println!("  {}Skipped: {}{}", YELLOW, summary.skipped, RESET);
    println!();

    if !summary.failures.is_empty() {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("{RED}Failure Details{RESET}");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        for (i, failure) in summary.failures.iter().enumerate() {
            println!();
            println!(
                "{}. Imposter :{} {} - Stub #{}{}",
                i + 1,
                failure.imposter_port,
                failure
                    .imposter_name
                    .as_ref()
                    .map(|n| format!("({n})"))
                    .unwrap_or_default(),
                failure.stub_index,
                failure
                    .stub_id
                    .as_ref()
                    .map(|id| format!(" [{id}]"))
                    .unwrap_or_default()
            );
            println!("   Request:  {}", failure.test_description);
            println!("   Expected: {}", failure.expected);
            println!("   {}Actual:   {}{}", RED, failure.actual, RESET);

            if show_curl {
                if let Some(ref curl) = failure.curl_command {
                    println!("   Curl:     {curl}");
                }
            }
        }
        println!();
    }

    // Final status
    if summary.failed == 0 {
        println!("{GREEN}All tests passed!{RESET}");
    } else {
        println!(
            "{}{} test(s) failed. See details above.{}",
            RED, summary.failed, RESET
        );
    }
}

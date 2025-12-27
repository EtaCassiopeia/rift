//! Rift Imposter Configuration Linter
//!
//! This tool validates imposter configuration files for compatibility with Rift,
//! detecting common issues before loading them into the server.
//!
//! Usage:
//!   rift-lint <directory_or_file> [OPTIONS]
//!
//! Features:
//! - Port conflict detection across multiple imposter files
//! - Header value validation (must be strings)
//! - Status code validation
//! - JavaScript syntax validation for behaviors
//! - JSONPath selector validation
//! - Regex pattern validation
//! - Response structure validation
//! - Predicate structure validation

use clap::Parser;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ANSI color codes
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Rift Imposter Configuration Linter
#[derive(Parser, Debug)]
#[command(name = "rift-lint")]
#[command(
    author,
    version,
    about = "Validate imposter configuration files for Rift compatibility"
)]
struct Args {
    /// Path to imposter file or directory containing imposter files
    #[arg(required = true)]
    path: PathBuf,

    /// Fix issues automatically where possible
    #[arg(short, long)]
    fix: bool,

    /// Output format: text (default), json
    #[arg(short, long, default_value = "text")]
    output: String,

    /// Only show errors (hide warnings)
    #[arg(short = 'e', long)]
    errors_only: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Strict mode - treat warnings as errors
    #[arg(short, long)]
    strict: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    fn color(&self) -> &'static str {
        match self {
            Severity::Error => RED,
            Severity::Warning => YELLOW,
            Severity::Info => CYAN,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone)]
struct LintIssue {
    severity: Severity,
    code: String,
    message: String,
    file: PathBuf,
    location: Option<String>,
    suggestion: Option<String>,
}

#[derive(Debug, Default)]
struct LintResult {
    issues: Vec<LintIssue>,
    files_checked: usize,
    errors: usize,
    warnings: usize,
}

impl LintResult {
    fn add_issue(&mut self, issue: LintIssue) {
        match issue.severity {
            Severity::Error => self.errors += 1,
            Severity::Warning => self.warnings += 1,
            Severity::Info => {}
        }
        self.issues.push(issue);
    }
}

/// JavaScript syntax validator using boa_engine
#[cfg(feature = "javascript")]
mod js_validator {
    use boa_engine::{Context, Source};

    pub fn validate_javascript(script: &str) -> Result<(), String> {
        let mut context = Context::default();

        // Mountebank uses function expressions that need to be wrapped
        // e.g., "function() { return 0; }" needs to be "(function() { return 0; })"
        let script_trimmed = script.trim();
        let wrapped =
            if script_trimmed.starts_with("function") && !script_trimmed.contains("function ") {
                // Anonymous function expression - wrap in parentheses and assign to variable
                format!("var __fn = ({script_trimmed})")
            } else {
                script_trimmed.to_string()
            };

        // Try to parse the script
        match context.eval(Source::from_bytes(&wrapped)) {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                // Filter out runtime errors - we only care about syntax
                if err_str.contains("SyntaxError") || err_str.contains("unexpected") {
                    Err(err_str)
                } else {
                    // Runtime errors are okay (undefined variables, etc.)
                    Ok(())
                }
            }
        }
    }
}

#[cfg(not(feature = "javascript"))]
mod js_validator {
    pub fn validate_javascript(_script: &str) -> Result<(), String> {
        // Basic syntax check without boa_engine
        // Check for obvious issues
        Ok(())
    }
}

fn main() {
    let args = Args::parse();

    println!("{BOLD}{CYAN}Rift Imposter Linter{RESET}");
    println!("{DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━{RESET}");

    let mut result = LintResult::default();

    // Collect all imposter files
    let files = collect_imposter_files(&args.path);

    if files.is_empty() {
        println!(
            "{YELLOW}Warning:{RESET} No JSON files found in {:?}",
            args.path
        );
        std::process::exit(0);
    }

    println!("{DIM}Scanning:{RESET} {CYAN}{}{RESET}", args.path.display());
    println!(
        "{DIM}Found:{RESET}    {BOLD}{}{RESET} imposter file(s)\n",
        files.len()
    );
    result.files_checked = files.len();

    // First pass: Load all files and check for port conflicts
    let mut port_map: HashMap<u16, Vec<PathBuf>> = HashMap::new();
    let mut imposters: Vec<(PathBuf, Value)> = Vec::new();

    for file in &files {
        match load_imposter_file(file) {
            Ok(imposter) => {
                if let Some(port) = imposter.get("port").and_then(|v| v.as_u64()) {
                    port_map.entry(port as u16).or_default().push(file.clone());
                }
                imposters.push((file.clone(), imposter));
            }
            Err(e) => {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E001".to_string(),
                    message: format!("Failed to parse JSON: {e}"),
                    file: file.clone(),
                    location: None,
                    suggestion: Some("Check for JSON syntax errors".to_string()),
                });
            }
        }
    }

    // Check for port conflicts
    check_port_conflicts(&port_map, &mut result);

    // Second pass: Validate each imposter
    for (file, imposter) in &imposters {
        validate_imposter(file, imposter, &mut result, &args);
    }

    // Print results
    if args.output == "json" {
        print_results_json(&result);
    } else {
        print_results(&result, &args);
    }

    // Apply fixes if requested
    if args.fix && result.errors > 0 {
        println!("\n{BOLD}Applying fixes...{RESET}");
        apply_fixes(&imposters, &result);
    }

    // Exit with error code if there were errors (or warnings in strict mode)
    let has_errors = result.errors > 0 || (args.strict && result.warnings > 0);
    std::process::exit(if has_errors { 1 } else { 0 });
}

fn collect_imposter_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path.to_path_buf());
        }
    } else if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "json") {
                    files.push(entry_path);
                }
            }
        }
    }

    files.sort();
    files
}

fn load_imposter_file(path: &Path) -> Result<Value, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn check_port_conflicts(port_map: &HashMap<u16, Vec<PathBuf>>, result: &mut LintResult) {
    for (port, files) in port_map {
        if files.len() > 1 {
            let file_names: Vec<String> = files
                .iter()
                .map(|f| {
                    f.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                })
                .collect();

            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E002".to_string(),
                message: format!(
                    "Port {port} is used by {} files: {}",
                    files.len(),
                    file_names.join(", ")
                ),
                file: files[0].clone(),
                location: Some("port".to_string()),
                suggestion: Some(format!(
                    "Assign unique ports to each imposter. Consider using ports {}+",
                    port + 1
                )),
            });
        }
    }
}

fn validate_imposter(file: &Path, imposter: &Value, result: &mut LintResult, args: &Args) {
    // Check required fields
    check_required_fields(file, imposter, result);

    // Check protocol
    check_protocol(file, imposter, result);

    // Check port range
    check_port_range(file, imposter, result);

    // Validate stubs
    if let Some(stubs) = imposter.get("stubs").and_then(|v| v.as_array()) {
        for (idx, stub) in stubs.iter().enumerate() {
            validate_stub(file, stub, idx, result, args);
        }
    }
}

fn check_required_fields(file: &Path, imposter: &Value, result: &mut LintResult) {
    let required = ["port", "protocol", "stubs"];

    for field in required {
        if imposter.get(field).is_none() {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E003".to_string(),
                message: format!("Missing required field: {field}"),
                file: file.to_path_buf(),
                location: None,
                suggestion: Some(format!("Add \"{field}\" to the imposter configuration")),
            });
        }
    }
}

fn check_protocol(file: &Path, imposter: &Value, result: &mut LintResult) {
    if let Some(protocol) = imposter.get("protocol").and_then(|v| v.as_str()) {
        if !["http", "https", "tcp"].contains(&protocol) {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E004".to_string(),
                message: format!("Invalid protocol: {protocol}"),
                file: file.to_path_buf(),
                location: Some("protocol".to_string()),
                suggestion: Some("Use 'http', 'https', or 'tcp'".to_string()),
            });
        }
    }
}

fn check_port_range(file: &Path, imposter: &Value, result: &mut LintResult) {
    if let Some(port) = imposter.get("port").and_then(|v| v.as_u64()) {
        if !(1..=65535).contains(&port) {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E005".to_string(),
                message: format!("Port {port} is out of valid range (1-65535)"),
                file: file.to_path_buf(),
                location: Some("port".to_string()),
                suggestion: None,
            });
        } else if port < 1024 {
            result.add_issue(LintIssue {
                severity: Severity::Warning,
                code: "W001".to_string(),
                message: format!("Port {port} is a privileged port (requires root)"),
                file: file.to_path_buf(),
                location: Some("port".to_string()),
                suggestion: Some("Consider using a port >= 1024".to_string()),
            });
        }
    }
}

fn validate_stub(file: &Path, stub: &Value, idx: usize, result: &mut LintResult, args: &Args) {
    let location = format!("stubs[{idx}]");

    // Validate predicates
    if let Some(predicates) = stub.get("predicates").and_then(|v| v.as_array()) {
        for (pred_idx, predicate) in predicates.iter().enumerate() {
            validate_predicate(
                file,
                predicate,
                &format!("{location}.predicates[{pred_idx}]"),
                result,
                args,
            );
        }
    }

    // Validate responses
    if let Some(responses) = stub.get("responses").and_then(|v| v.as_array()) {
        if responses.is_empty() {
            result.add_issue(LintIssue {
                severity: Severity::Warning,
                code: "W002".to_string(),
                message: "Stub has no responses defined".to_string(),
                file: file.to_path_buf(),
                location: Some(location.clone()),
                suggestion: Some("Add at least one response".to_string()),
            });
        }

        for (resp_idx, response) in responses.iter().enumerate() {
            validate_response(
                file,
                response,
                &format!("{location}.responses[{resp_idx}]"),
                result,
                args,
            );
        }
    } else {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E006".to_string(),
            message: "Stub missing 'responses' field".to_string(),
            file: file.to_path_buf(),
            location: Some(location),
            suggestion: None,
        });
    }
}

fn validate_predicate(
    file: &Path,
    predicate: &Value,
    location: &str,
    result: &mut LintResult,
    args: &Args,
) {
    let valid_operators = [
        "equals",
        "deepEquals",
        "contains",
        "startsWith",
        "endsWith",
        "matches",
        "exists",
        "not",
        "or",
        "and",
        "inject",
    ];

    // Check for valid operator
    let pred_obj = match predicate.as_object() {
        Some(obj) => obj,
        None => {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E007".to_string(),
                message: "Predicate must be an object".to_string(),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: None,
            });
            return;
        }
    };

    // Find the operator key (ignoring modifiers like jsonpath, caseSensitive)
    let modifier_keys: HashSet<&str> = ["jsonpath", "xpath", "caseSensitive", "except"]
        .into_iter()
        .collect();
    let operator_keys: Vec<&String> = pred_obj
        .keys()
        .filter(|k| !modifier_keys.contains(k.as_str()))
        .collect();

    if operator_keys.is_empty() {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E008".to_string(),
            message: "Predicate has no operator".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: Some(format!("Add one of: {}", valid_operators.join(", "))),
        });
        return;
    }

    for operator in &operator_keys {
        if !valid_operators.contains(&operator.as_str()) {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E009".to_string(),
                message: format!("Unknown predicate operator: {operator}"),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: Some(format!("Use one of: {}", valid_operators.join(", "))),
            });
        }
    }

    // Validate jsonpath selectors
    if let Some(jsonpath) = predicate.get("jsonpath") {
        validate_jsonpath(file, jsonpath, location, result);
    }

    // Validate regex patterns in matches
    if let Some(matches) = predicate.get("matches") {
        validate_regex_patterns(file, matches, location, result, args);
    }

    // Recursively validate nested predicates
    for key in ["and", "or", "not"] {
        if let Some(nested) = predicate.get(key) {
            if key == "not" {
                if let Some(nested_pred) = nested.as_object() {
                    validate_predicate(
                        file,
                        &Value::Object(nested_pred.clone()),
                        &format!("{location}.not"),
                        result,
                        args,
                    );
                }
            } else if let Some(nested_array) = nested.as_array() {
                for (i, nested_pred) in nested_array.iter().enumerate() {
                    validate_predicate(
                        file,
                        nested_pred,
                        &format!("{location}.{key}[{i}]"),
                        result,
                        args,
                    );
                }
            }
        }
    }
}

fn validate_jsonpath(file: &Path, jsonpath: &Value, location: &str, result: &mut LintResult) {
    if let Some(selector) = jsonpath.get("selector").and_then(|v| v.as_str()) {
        // Check for Mountebank's non-standard slice notation [:N]
        let slice_re = Regex::new(r"\[:(\d+)\]").unwrap();
        if slice_re.is_match(selector) {
            result.add_issue(LintIssue {
                severity: Severity::Info,
                code: "I001".to_string(),
                message: format!("JSONPath uses Mountebank slice notation: {selector}"),
                file: file.to_path_buf(),
                location: Some(format!("{location}.jsonpath.selector")),
                suggestion: Some("This is supported by Rift but not standard JSONPath".to_string()),
            });
        }

        // Check for unbalanced brackets
        let open_brackets = selector.chars().filter(|c| *c == '[').count();
        let close_brackets = selector.chars().filter(|c| *c == ']').count();
        if open_brackets != close_brackets {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E010".to_string(),
                message: "Unbalanced brackets in JSONPath selector".to_string(),
                file: file.to_path_buf(),
                location: Some(format!("{location}.jsonpath.selector")),
                suggestion: None,
            });
        }
    } else {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E011".to_string(),
            message: "JSONPath missing 'selector' field".to_string(),
            file: file.to_path_buf(),
            location: Some(format!("{location}.jsonpath")),
            suggestion: None,
        });
    }
}

fn validate_regex_patterns(
    file: &Path,
    matches: &Value,
    location: &str,
    result: &mut LintResult,
    args: &Args,
) {
    if let Some(obj) = matches.as_object() {
        for (field, pattern) in obj {
            if let Some(pattern_str) = pattern.as_str() {
                match Regex::new(pattern_str) {
                    Ok(_) => {
                        if args.verbose {
                            println!("  {DIM}Validated regex: {pattern_str}{RESET}");
                        }
                    }
                    Err(e) => {
                        result.add_issue(LintIssue {
                            severity: Severity::Error,
                            code: "E013".to_string(),
                            message: format!("Invalid regex pattern in '{field}': {e}"),
                            file: file.to_path_buf(),
                            location: Some(format!("{location}.matches.{field}")),
                            suggestion: Some("Check regex syntax".to_string()),
                        });
                    }
                }
            }
        }
    }
}

fn validate_response(
    file: &Path,
    response: &Value,
    location: &str,
    result: &mut LintResult,
    args: &Args,
) {
    let has_is = response.get("is").is_some();
    let has_proxy = response
        .get("proxy")
        .map(|p| !p.is_null() && p.is_object() && p.get("to").is_some())
        .unwrap_or(false);
    let has_inject = response.get("inject").is_some();
    let has_fault = response.get("fault").is_some();

    // Check response type - should have exactly one of: is, proxy, inject, fault
    let response_types = [has_is, has_proxy, has_inject, has_fault];
    let active_types = response_types.iter().filter(|&&t| t).count();

    if active_types == 0 {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E014".to_string(),
            message: "Response has no response type (is, proxy, inject, or fault)".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: Some(
                "Add 'is', 'proxy', 'inject', or 'fault' to define the response".to_string(),
            ),
        });
    } else if active_types > 1 && has_is && has_proxy {
        // Having both "is" and "proxy" is allowed if proxy is null
        let proxy_val = response.get("proxy");
        if proxy_val.map(|p| !p.is_null()).unwrap_or(false) {
            result.add_issue(LintIssue {
                severity: Severity::Warning,
                code: "W003".to_string(),
                message: "Response has both 'is' and 'proxy' defined".to_string(),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: Some(
                    "Use either 'is' for static responses or 'proxy' for forwarding".to_string(),
                ),
            });
        }
    }

    // Validate "is" response
    if let Some(is_response) = response.get("is") {
        validate_is_response(file, is_response, &format!("{location}.is"), result);
    }

    // Validate proxy
    if let Some(proxy) = response.get("proxy") {
        if !proxy.is_null() {
            validate_proxy_response(file, proxy, &format!("{location}.proxy"), result);
        }
    }

    // Validate behaviors
    if let Some(behaviors) = response.get("behaviors").and_then(|v| v.as_array()) {
        for (idx, behavior) in behaviors.iter().enumerate() {
            validate_behavior(
                file,
                behavior,
                &format!("{location}.behaviors[{idx}]"),
                result,
                args,
            );
        }
    }
}

fn validate_is_response(file: &Path, is_response: &Value, location: &str, result: &mut LintResult) {
    // Validate status code
    if let Some(status) = is_response.get("statusCode") {
        let status_num = status
            .as_u64()
            .or_else(|| status.as_str().and_then(|s| s.parse().ok()));

        match status_num {
            Some(code) if !(100..=599).contains(&code) => {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E015".to_string(),
                    message: format!("Invalid HTTP status code: {code}"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.statusCode")),
                    suggestion: Some("Use a valid HTTP status code (100-599)".to_string()),
                });
            }
            None => {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E016".to_string(),
                    message: "statusCode must be a number or numeric string".to_string(),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.statusCode")),
                    suggestion: None,
                });
            }
            _ => {}
        }
    }

    // Validate headers
    if let Some(headers) = is_response.get("headers") {
        validate_headers(file, headers, &format!("{location}.headers"), result);
    }

    // Check if body is valid JSON when Content-Type is application/json
    if let Some(body) = is_response.get("body") {
        if let Some(headers) = is_response.get("headers").and_then(|h| h.as_object()) {
            let content_type = headers
                .iter()
                .find(|(k, _)| k.to_lowercase() == "content-type")
                .and_then(|(_, v)| v.as_str());

            if content_type
                .map(|ct| ct.contains("application/json"))
                .unwrap_or(false)
            {
                if let Some(body_str) = body.as_str() {
                    if serde_json::from_str::<Value>(body_str).is_err() {
                        result.add_issue(LintIssue {
                            severity: Severity::Warning,
                            code: "W004".to_string(),
                            message: "Body is not valid JSON but Content-Type is application/json"
                                .to_string(),
                            file: file.to_path_buf(),
                            location: Some(format!("{location}.body")),
                            suggestion: Some("Verify the body is valid JSON".to_string()),
                        });
                    }
                }
            }
        }
    }
}

fn validate_headers(file: &Path, headers: &Value, location: &str, result: &mut LintResult) {
    if let Some(headers_obj) = headers.as_object() {
        for (name, value) in headers_obj {
            // Check header name is valid
            if name.is_empty() {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E017".to_string(),
                    message: "Empty header name".to_string(),
                    file: file.to_path_buf(),
                    location: Some(location.to_string()),
                    suggestion: None,
                });
            }

            // Check header value is a string
            if value.is_array() {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E018".to_string(),
                    message: format!("Header '{name}' value is an array, must be a string"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.{name}")),
                    suggestion: Some("Convert array to comma-separated string".to_string()),
                });
            } else if value.is_number() {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E019".to_string(),
                    message: format!("Header '{name}' value is a number, must be a string"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.{name}")),
                    suggestion: Some(format!("Change to: \"{name}\": \"{}\"", value)),
                });
            } else if value.is_boolean() {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E020".to_string(),
                    message: format!("Header '{name}' value is a boolean, must be a string"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.{name}")),
                    suggestion: Some(format!("Change to: \"{name}\": \"{}\"", value)),
                });
            } else if value.is_null() {
                result.add_issue(LintIssue {
                    severity: Severity::Warning,
                    code: "W005".to_string(),
                    message: format!("Header '{name}' value is null"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.{name}")),
                    suggestion: Some("Remove header or set a string value".to_string()),
                });
            }

            // Check for common Content-Length issues
            if name.to_lowercase() == "content-length" {
                if let Some(len_str) = value.as_str() {
                    if let Ok(len) = len_str.parse::<u64>() {
                        if len < 10 {
                            result.add_issue(LintIssue {
                                severity: Severity::Warning,
                                code: "W006".to_string(),
                                message: format!(
                                    "Content-Length is very small ({len}), may cause issues"
                                ),
                                file: file.to_path_buf(),
                                location: Some(format!("{location}.{name}")),
                                suggestion: Some(
                                    "Verify Content-Length matches actual body length".to_string(),
                                ),
                            });
                        }
                    }
                }
            }
        }
    } else {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E021".to_string(),
            message: "Headers must be an object".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: None,
        });
    }
}

fn validate_proxy_response(file: &Path, proxy: &Value, location: &str, result: &mut LintResult) {
    if let Some(to) = proxy.get("to") {
        if let Some(url) = to.as_str() {
            // Basic URL validation
            if !url.starts_with("http://") && !url.starts_with("https://") {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E022".to_string(),
                    message: format!("Proxy 'to' URL must start with http:// or https://: {url}"),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.to")),
                    suggestion: None,
                });
            }

            // Check for localhost with unreachable port patterns
            if url.contains("localhost:") || url.contains("127.0.0.1:") {
                let port_re = Regex::new(r":(\d+)").unwrap();
                if let Some(captures) = port_re.captures(url) {
                    if let Ok(port) = captures[1].parse::<u16>() {
                        if port > 10000 {
                            result.add_issue(LintIssue {
                                severity: Severity::Info,
                                code: "I002".to_string(),
                                message: format!("Proxy targets localhost:{port}"),
                                file: file.to_path_buf(),
                                location: Some(format!("{location}.to")),
                                suggestion: Some(
                                    "Ensure upstream service is running on this port".to_string(),
                                ),
                            });
                        }
                    }
                }
            }
        } else {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E023".to_string(),
                message: "Proxy 'to' must be a string URL".to_string(),
                file: file.to_path_buf(),
                location: Some(format!("{location}.to")),
                suggestion: None,
            });
        }
    } else {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E024".to_string(),
            message: "Proxy missing required 'to' field".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: None,
        });
    }

    // Validate proxy mode
    if let Some(mode) = proxy.get("mode").and_then(|v| v.as_str()) {
        let valid_modes = ["proxyOnce", "proxyAlways", "proxyTransparent"];
        if !valid_modes.contains(&mode) {
            result.add_issue(LintIssue {
                severity: Severity::Warning,
                code: "W007".to_string(),
                message: format!("Unknown proxy mode: {mode}"),
                file: file.to_path_buf(),
                location: Some(format!("{location}.mode")),
                suggestion: Some(format!("Use one of: {}", valid_modes.join(", "))),
            });
        }
    }
}

fn validate_behavior(
    file: &Path,
    behavior: &Value,
    location: &str,
    result: &mut LintResult,
    args: &Args,
) {
    if let Some(obj) = behavior.as_object() {
        // Validate wait behavior (JavaScript)
        if let Some(wait) = obj.get("wait") {
            if let Some(script) = wait.as_str() {
                validate_javascript_behavior(
                    file,
                    script,
                    &format!("{location}.wait"),
                    result,
                    args,
                );
            } else if !wait.is_number() {
                result.add_issue(LintIssue {
                    severity: Severity::Error,
                    code: "E025".to_string(),
                    message: "Wait behavior must be a number or JavaScript function string"
                        .to_string(),
                    file: file.to_path_buf(),
                    location: Some(format!("{location}.wait")),
                    suggestion: None,
                });
            }
        }

        // Validate decorate behavior (JavaScript)
        if let Some(decorate) = obj.get("decorate") {
            if let Some(script) = decorate.as_str() {
                validate_javascript_behavior(
                    file,
                    script,
                    &format!("{location}.decorate"),
                    result,
                    args,
                );
            }
        }

        // Validate shellTransform behavior
        if let Some(shell) = obj.get("shellTransform") {
            if let Some(cmd) = shell.as_str() {
                // Check for potentially dangerous commands
                let dangerous_patterns = ["rm ", "rm -", "sudo ", "chmod ", "dd ", "> /dev/"];
                for pattern in dangerous_patterns {
                    if cmd.contains(pattern) {
                        result.add_issue(LintIssue {
                            severity: Severity::Warning,
                            code: "W008".to_string(),
                            message: format!(
                                "shellTransform contains potentially dangerous command: {pattern}"
                            ),
                            file: file.to_path_buf(),
                            location: Some(format!("{location}.shellTransform")),
                            suggestion: Some("Review this command for safety".to_string()),
                        });
                    }
                }
            }
        }

        // Validate copy behavior
        if let Some(copy) = obj.get("copy") {
            validate_copy_behavior(file, copy, &format!("{location}.copy"), result);
        }

        // Validate lookup behavior
        if let Some(lookup) = obj.get("lookup") {
            validate_lookup_behavior(file, lookup, &format!("{location}.lookup"), result);
        }
    }
}

fn validate_javascript_behavior(
    file: &Path,
    script: &str,
    location: &str,
    result: &mut LintResult,
    args: &Args,
) {
    // Quick syntax checks
    let script_trimmed = script.trim();

    // Check for function definition (Mountebank uses anonymous function expressions)
    if !script_trimmed.starts_with("function") && !script_trimmed.is_empty() {
        result.add_issue(LintIssue {
            severity: Severity::Warning,
            code: "W009".to_string(),
            message: "JavaScript behavior should be a function expression".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: Some("Wrap code in: function() { ... }".to_string()),
        });
    }

    // Check for balanced braces
    let open_braces = script.chars().filter(|c| *c == '{').count();
    let close_braces = script.chars().filter(|c| *c == '}').count();
    if open_braces != close_braces {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E026".to_string(),
            message: "Unbalanced braces in JavaScript".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: None,
        });
    }

    // Check for balanced parentheses
    let open_parens = script.chars().filter(|c| *c == '(').count();
    let close_parens = script.chars().filter(|c| *c == ')').count();
    if open_parens != close_parens {
        result.add_issue(LintIssue {
            severity: Severity::Error,
            code: "E027".to_string(),
            message: "Unbalanced parentheses in JavaScript".to_string(),
            file: file.to_path_buf(),
            location: Some(location.to_string()),
            suggestion: None,
        });
    }

    // Use boa_engine for deeper validation if available
    #[cfg(feature = "javascript")]
    {
        if let Err(e) = js_validator::validate_javascript(script) {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E028".to_string(),
                message: format!("JavaScript syntax error: {e}"),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: None,
            });
        }
    }

    if args.verbose {
        println!("  {DIM}Validated JavaScript at {location}{RESET}");
    }
}

fn validate_copy_behavior(file: &Path, copy: &Value, location: &str, result: &mut LintResult) {
    if let Some(arr) = copy.as_array() {
        for (idx, item) in arr.iter().enumerate() {
            if let Some(obj) = item.as_object() {
                if obj.get("from").is_none() {
                    result.add_issue(LintIssue {
                        severity: Severity::Error,
                        code: "E029".to_string(),
                        message: "Copy behavior item missing 'from' field".to_string(),
                        file: file.to_path_buf(),
                        location: Some(format!("{location}[{idx}]")),
                        suggestion: None,
                    });
                }
                if obj.get("into").is_none() {
                    result.add_issue(LintIssue {
                        severity: Severity::Error,
                        code: "E030".to_string(),
                        message: "Copy behavior item missing 'into' field".to_string(),
                        file: file.to_path_buf(),
                        location: Some(format!("{location}[{idx}]")),
                        suggestion: None,
                    });
                }
            }
        }
    }
}

fn validate_lookup_behavior(file: &Path, lookup: &Value, location: &str, result: &mut LintResult) {
    if let Some(obj) = lookup.as_object() {
        if obj.get("key").is_none() {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E031".to_string(),
                message: "Lookup behavior missing 'key' field".to_string(),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: None,
            });
        }
        if obj.get("fromDataSource").is_none() {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E032".to_string(),
                message: "Lookup behavior missing 'fromDataSource' field".to_string(),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: None,
            });
        }
        if obj.get("into").is_none() {
            result.add_issue(LintIssue {
                severity: Severity::Error,
                code: "E033".to_string(),
                message: "Lookup behavior missing 'into' field".to_string(),
                file: file.to_path_buf(),
                location: Some(location.to_string()),
                suggestion: None,
            });
        }
    }
}

fn print_results_json(result: &LintResult) {
    use serde_json::json;

    let issues: Vec<_> = result
        .issues
        .iter()
        .map(|issue| {
            json!({
                "severity": issue.severity.label(),
                "code": issue.code,
                "message": issue.message,
                "file": issue.file.to_string_lossy(),
                "location": issue.location,
                "suggestion": issue.suggestion
            })
        })
        .collect();

    let output = json!({
        "files_checked": result.files_checked,
        "errors": result.errors,
        "warnings": result.warnings,
        "issues": issues
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn print_results(result: &LintResult, args: &Args) {
    println!();

    if result.issues.is_empty() {
        println!("{GREEN}{BOLD}No issues found!{RESET}");
    } else {
        // Group issues by file
        let mut issues_by_file: HashMap<&PathBuf, Vec<&LintIssue>> = HashMap::new();
        for issue in &result.issues {
            issues_by_file.entry(&issue.file).or_default().push(issue);
        }

        // Sort files for consistent output
        let mut files: Vec<_> = issues_by_file.keys().collect();
        files.sort();

        for file in files {
            let issues = &issues_by_file[file];

            // Filter issues based on errors_only flag
            let filtered_issues: Vec<_> = if args.errors_only {
                issues
                    .iter()
                    .filter(|i| i.severity == Severity::Error)
                    .collect()
            } else {
                issues.iter().collect()
            };

            // Skip files with no relevant issues
            if filtered_issues.is_empty() {
                continue;
            }

            // Count errors and warnings for this file
            let file_errors = filtered_issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count();
            let file_warnings = filtered_issues
                .iter()
                .filter(|i| i.severity == Severity::Warning)
                .count();

            let file_name = file.file_name().unwrap_or_default().to_string_lossy();

            // File header with issue count
            let status_indicator = if file_errors > 0 {
                format!("{RED}FAIL{RESET}")
            } else {
                format!("{YELLOW}WARN{RESET}")
            };

            let counts = if file_errors > 0 && file_warnings > 0 {
                format!(
                    " {DIM}({RED}{file_errors} error(s){RESET}{DIM}, {YELLOW}{file_warnings} warning(s){RESET}{DIM}){RESET}"
                )
            } else if file_errors > 0 {
                format!(" {DIM}({RED}{file_errors} error(s){RESET}{DIM}){RESET}")
            } else if file_warnings > 0 {
                format!(" {DIM}({YELLOW}{file_warnings} warning(s){RESET}{DIM}){RESET}")
            } else {
                String::new()
            };

            println!("{status_indicator} {BOLD}{CYAN}{file_name}{RESET}{counts}");

            for issue in filtered_issues {
                let severity_marker = match issue.severity {
                    Severity::Error => format!("{RED}|{RESET}"),
                    Severity::Warning => format!("{YELLOW}|{RESET}"),
                    Severity::Info => format!("{CYAN}|{RESET}"),
                };

                let severity_str = format!(
                    "{BOLD}{}{}{RESET}",
                    issue.severity.color(),
                    issue.severity.label()
                );

                let location_str = issue
                    .location
                    .as_ref()
                    .map(|l| format!("{DIM}[{RESET}{CYAN}{l}{RESET}{DIM}]{RESET}"))
                    .unwrap_or_default();

                let code_str = format!(
                    "{DIM}({}{}{DIM}){RESET}",
                    issue.severity.color(),
                    issue.code
                );

                println!(
                    "  {severity_marker} {location_str} {severity_str}: {} {code_str}",
                    issue.message
                );

                if let Some(suggestion) = &issue.suggestion {
                    println!("  {severity_marker}   {GREEN}-> {suggestion}{RESET}");
                }
            }
            println!();
        }
    }

    // Summary
    println!("{DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━{RESET}");
    println!("{BOLD}{CYAN}Summary{RESET}");
    println!("{DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━{RESET}");
    println!(
        "  {DIM}Files checked:{RESET} {BOLD}{}{RESET}",
        result.files_checked
    );

    // Errors count
    if result.errors > 0 {
        println!(
            "  {RED}Errors:{RESET}    {BOLD}{RED}{}{RESET}",
            result.errors
        );
    } else {
        println!("  {GREEN}Errors:{RESET}    {BOLD}{GREEN}0{RESET}");
    }

    // Warnings count
    if result.warnings > 0 {
        println!(
            "  {YELLOW}Warnings:{RESET}  {BOLD}{YELLOW}{}{RESET}",
            result.warnings
        );
    } else {
        println!("  {DIM}Warnings:{RESET}  {BOLD}0{RESET}");
    }

    println!();

    if result.errors == 0 && result.warnings == 0 {
        println!("{GREEN}{BOLD}All checks passed!{RESET}");
    } else if result.errors == 0 {
        println!("{YELLOW}{BOLD}Passed with warnings{RESET}");
    } else {
        println!("{RED}{BOLD}Linting failed with errors{RESET}");
    }
}

fn apply_fixes(imposters: &[(PathBuf, Value)], _result: &LintResult) {
    let mut fixes_applied = 0;

    for (file, imposter) in imposters {
        let mut modified = imposter.clone();
        let mut file_fixed = false;

        // Fix header values
        if let Some(stubs) = modified.get_mut("stubs").and_then(|v| v.as_array_mut()) {
            for stub in stubs {
                if let Some(responses) = stub.get_mut("responses").and_then(|v| v.as_array_mut()) {
                    for response in responses {
                        if let Some(is_response) = response.get_mut("is") {
                            if let Some(headers) = is_response
                                .get_mut("headers")
                                .and_then(|v| v.as_object_mut())
                            {
                                for (name, value) in headers.iter_mut() {
                                    if value.is_array() {
                                        // Convert array to comma-separated string
                                        if let Some(arr) = value.as_array() {
                                            let joined: Vec<String> = arr
                                                .iter()
                                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                                .collect();
                                            *value = Value::String(joined.join(", "));
                                            file_fixed = true;
                                            fixes_applied += 1;
                                            println!("  Fixed header '{name}' array → string");
                                        }
                                    } else if value.is_number() {
                                        *value = Value::String(value.to_string());
                                        file_fixed = true;
                                        fixes_applied += 1;
                                        println!("  Fixed header '{name}' number → string");
                                    } else if value.is_boolean() {
                                        let bool_str = if value.as_bool().unwrap_or(false) {
                                            "true"
                                        } else {
                                            "false"
                                        };
                                        *value = Value::String(bool_str.to_string());
                                        file_fixed = true;
                                        fixes_applied += 1;
                                        println!("  Fixed header '{name}' boolean → string");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Write fixed file
        if file_fixed {
            match serde_json::to_string_pretty(&modified) {
                Ok(content) => {
                    if let Err(e) = std::fs::write(file, content) {
                        println!("{RED}Error writing {}: {e}{RESET}", file.display());
                    } else {
                        println!("{GREEN}Fixed: {}{RESET}", file.display());
                    }
                }
                Err(e) => {
                    println!("{RED}Error serializing {}: {e}{RESET}", file.display());
                }
            }
        }
    }

    println!("\n{GREEN}Applied {fixes_applied} fixes{RESET}");
}

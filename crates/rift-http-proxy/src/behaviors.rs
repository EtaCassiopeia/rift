//! Mountebank-compatible response behaviors.
//!
//! This module implements the `_behaviors` functionality from Mountebank,
//! allowing dynamic response modification based on request data.
//!
//! # Supported Behaviors
//!
//! - `wait` - Add latency before response (fixed ms or {min, max} range)
//! - `repeat` - Repeat response N times before cycling to next
//! - `copy` - Copy request fields into response using regex/jsonpath/xpath
//! - `lookup` - Query external CSV data source

// Allow dead code for now as behaviors are designed for future integration
#![allow(dead_code)]

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

// =============================================================================
// CONFIGURATION TYPES
// =============================================================================

/// Response behaviors that modify how responses are generated
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseBehaviors {
    /// Add latency before response
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitBehavior>,

    /// Repeat response N times before advancing to next
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u32>,

    /// Copy fields from request to response
    /// Mountebank allows both single object and array format
    #[serde(
        default,
        deserialize_with = "deserialize_copy_behaviors",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub copy: Vec<CopyBehavior>,

    /// Lookup from external data source
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lookup: Vec<LookupBehavior>,

    /// Shell transform - external program transforms response
    /// The program receives MB_REQUEST and MB_RESPONSE env vars
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_transform: Option<String>,

    /// Decorate - Rhai script to post-process response (Mountebank-compatible)
    /// Script receives `request` and `response` variables and can modify response
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorate: Option<String>,
}

/// Custom deserializer for copy behaviors that accepts both object and array
fn deserialize_copy_behaviors<'de, D>(deserializer: D) -> Result<Vec<CopyBehavior>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct CopyBehaviorsVisitor;

    impl<'de> Visitor<'de> for CopyBehaviorsVisitor {
        type Value = Vec<CopyBehavior>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a copy behavior object or array of copy behaviors")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut behaviors = Vec::new();
            while let Some(behavior) = seq.next_element()? {
                behaviors.push(behavior);
            }
            Ok(behaviors)
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            // Single object - wrap in vec
            let behavior = CopyBehavior::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![behavior])
        }
    }

    deserializer.deserialize_any(CopyBehaviorsVisitor)
}

/// Wait behavior - add latency before response
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WaitBehavior {
    /// Fixed delay in milliseconds
    Fixed(u64),
    /// Random delay within range
    Range {
        #[serde(rename = "min")]
        min_ms: u64,
        #[serde(rename = "max")]
        max_ms: u64,
    },
    /// JavaScript function that returns delay
    Function(String),
}

impl WaitBehavior {
    /// Get the wait duration in milliseconds
    pub fn get_duration_ms(&self) -> u64 {
        match self {
            WaitBehavior::Fixed(ms) => *ms,
            WaitBehavior::Range { min_ms, max_ms } => {
                use rand::Rng;
                rand::thread_rng().gen_range(*min_ms..=*max_ms)
            }
            WaitBehavior::Function(js_func) => {
                // Parse JavaScript function and execute
                // Format: "function() { return Math.floor(Math.random() * 100) + 50; }"
                Self::execute_js_wait_function(js_func).unwrap_or(100)
            }
        }
    }

    /// Execute a JavaScript wait function
    fn execute_js_wait_function(js_func: &str) -> Option<u64> {
        // Extract the function body
        let trimmed = js_func.trim();
        if !trimmed.starts_with("function") {
            return None;
        }

        // Parse simple patterns:
        // Math.floor(Math.random() * N) + M -> random between M and M+N
        if let Some(body) = extract_function_body(trimmed) {
            // Look for patterns like "Math.floor(Math.random() * 100) + 50"
            // or "return Math.floor(Math.random() * 100) + 50;"
            let body = body
                .replace("return ", "")
                .trim_end_matches(';')
                .to_string();

            // Parse: Math.floor(Math.random() * N) + M
            if body.contains("Math.random()") {
                use rand::Rng;
                // Extract multiplier and offset using regex
                let re = regex::Regex::new(
                    r"Math\.floor\s*\(\s*Math\.random\s*\(\s*\)\s*\*\s*(\d+)\s*\)\s*\+\s*(\d+)",
                )
                .ok()?;

                if let Some(caps) = re.captures(&body) {
                    let range = caps.get(1)?.as_str().parse::<u64>().ok()?;
                    let offset = caps.get(2)?.as_str().parse::<u64>().ok()?;
                    return Some(rand::thread_rng().gen_range(offset..=offset + range));
                }

                // Simpler pattern: Math.random() * N
                let re = regex::Regex::new(r"Math\.random\s*\(\s*\)\s*\*\s*(\d+)").ok()?;
                if let Some(caps) = re.captures(&body) {
                    let range = caps.get(1)?.as_str().parse::<u64>().ok()?;
                    return Some(rand::thread_rng().gen_range(0..=range));
                }
            }

            // Try to parse as simple number
            body.trim().parse::<u64>().ok()
        } else {
            None
        }
    }
}

/// Extract function body from JavaScript function string
fn extract_function_body(js_func: &str) -> Option<String> {
    let start = js_func.find('{')?;
    let end = js_func.rfind('}')?;
    if start < end {
        Some(js_func[start + 1..end].trim().to_string())
    } else {
        None
    }
}

/// Copy behavior - copy request fields into response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CopyBehavior {
    /// Request field to copy from
    pub from: CopySource,
    /// Response token to replace (e.g., "${NAME}")
    pub into: String,
    /// Extraction method
    #[serde(rename = "using")]
    pub extraction: ExtractionMethod,
}

/// Source of data to copy from request
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CopySource {
    /// Simple field: "path", "body", "method"
    Simple(String),
    /// Nested field: {"query": "name"} or {"headers": "Content-Type"}
    Nested(HashMap<String, String>),
}

impl CopySource {
    /// Extract value from request data
    pub fn extract(&self, request: &RequestContext) -> Option<String> {
        match self {
            CopySource::Simple(field) => match field.as_str() {
                "path" => Some(request.path.clone()),
                "method" => Some(request.method.clone()),
                "body" => request.body.clone(),
                _ => None,
            },
            CopySource::Nested(map) => {
                if let Some(param_name) = map.get("query") {
                    request.query.get(param_name).cloned()
                } else if let Some(header_name) = map.get("headers") {
                    // Case-insensitive header lookup since HTTP headers are case-insensitive
                    let lower_name = header_name.to_lowercase();
                    request
                        .headers
                        .iter()
                        .find(|(k, _)| k.to_lowercase() == lower_name)
                        .map(|(_, v)| v.clone())
                } else {
                    None
                }
            }
        }
    }
}

/// Method for extracting values from source
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum ExtractionMethod {
    /// Regular expression with capture groups
    Regex { selector: String },
    /// JSONPath expression
    #[serde(rename = "jsonpath")]
    JsonPath { selector: String },
    /// XPath expression for XML
    #[serde(rename = "xpath")]
    XPath { selector: String },
}

impl ExtractionMethod {
    /// Apply extraction to a value
    pub fn extract(&self, value: &str) -> Option<String> {
        match self {
            ExtractionMethod::Regex { selector } => {
                let re = Regex::new(selector).ok()?;
                if let Some(caps) = re.captures(value) {
                    // Return first capture group if exists, otherwise full match
                    caps.get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str().to_string())
                } else {
                    None
                }
            }
            ExtractionMethod::JsonPath { selector } => extract_jsonpath(value, selector),
            ExtractionMethod::XPath { selector } => extract_xpath(value, selector),
        }
    }
}

/// Lookup behavior - query external data source
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LookupBehavior {
    /// Key extraction from request
    pub key: LookupKey,
    /// Data source configuration
    #[serde(rename = "fromDataSource")]
    pub from_data_source: DataSource,
    /// Token to replace in response (e.g., "${RESULT}")
    pub into: String,
}

/// Key extraction configuration for lookup
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LookupKey {
    /// Request field to extract key from
    pub from: CopySource,
    /// Extraction method
    #[serde(rename = "using")]
    pub extraction: ExtractionMethod,
}

/// External data source configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DataSource {
    /// CSV data source
    pub csv: CsvDataSource,
}

/// CSV data source configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CsvDataSource {
    /// Path to CSV file
    pub path: String,
    /// Column to use as lookup key
    #[serde(rename = "keyColumn")]
    pub key_column: String,
    /// Delimiter character (default: ',')
    #[serde(default = "default_delimiter")]
    pub delimiter: char,
}

fn default_delimiter() -> char {
    ','
}

// =============================================================================
// REQUEST CONTEXT
// =============================================================================

/// Request context for behavior processing
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

impl RequestContext {
    /// Create from hyper request parts
    pub fn from_request(
        method: &str,
        uri: &hyper::Uri,
        headers: &hyper::HeaderMap,
        body: Option<&str>,
    ) -> Self {
        let mut query_map = HashMap::new();
        if let Some(query) = uri.query() {
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    query_map.insert(
                        key.to_string(),
                        urlencoding::decode(value).unwrap_or_default().to_string(),
                    );
                }
            }
        }

        let mut header_map = HashMap::new();
        for (name, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                // Preserve header case for Mountebank compatibility
                // Convert from hyper's lowercase to title case (e.g., "content-type" -> "Content-Type")
                let title_case_name = name
                    .as_str()
                    .split('-')
                    .map(|part| {
                        let mut chars: Vec<char> = part.chars().collect();
                        if let Some(first) = chars.first_mut() {
                            *first = first.to_uppercase().next().unwrap_or(*first);
                        }
                        chars.into_iter().collect::<String>()
                    })
                    .collect::<Vec<String>>()
                    .join("-");
                header_map.insert(title_case_name, v.to_string());
            }
        }

        Self {
            method: method.to_string(),
            path: uri.path().to_string(),
            query: query_map,
            headers: header_map,
            body: body.map(|s| s.to_string()),
        }
    }
}

// =============================================================================
// RESPONSE CYCLING
// =============================================================================

/// Tracks response cycling state per rule
pub struct ResponseCycler {
    /// Current response index per rule
    indices: RwLock<HashMap<String, AtomicUsize>>,
    /// Repeat counters per rule (how many times current response has been used)
    repeat_counters: RwLock<HashMap<String, AtomicUsize>>,
}

impl Default for ResponseCycler {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseCycler {
    pub fn new() -> Self {
        Self {
            indices: RwLock::new(HashMap::new()),
            repeat_counters: RwLock::new(HashMap::new()),
        }
    }

    /// Get current response index for a rule, handling repeat behavior
    /// Returns the index and whether it advanced to a new response
    pub fn get_response_index(
        &self,
        rule_id: &str,
        response_count: usize,
        repeat: Option<u32>,
    ) -> usize {
        if response_count == 0 {
            return 0;
        }

        let repeat_count = repeat.unwrap_or(1).max(1) as usize;

        // Get or create the index and counter for this rule
        let indices = self.indices.read();
        let counters = self.repeat_counters.read();

        let current_index = indices
            .get(rule_id)
            .map(|i| i.load(Ordering::SeqCst))
            .unwrap_or(0);

        let _current_repeat = counters
            .get(rule_id)
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(0);

        // Drop read locks
        drop(indices);
        drop(counters);

        // Increment repeat counter
        let mut counters = self.repeat_counters.write();
        let counter = counters
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let new_repeat = counter.fetch_add(1, Ordering::SeqCst) + 1;

        // Check if we need to advance to next response
        if new_repeat >= repeat_count {
            // Reset repeat counter
            counter.store(0, Ordering::SeqCst);

            // Advance to next response
            let mut indices = self.indices.write();
            let index = indices
                .entry(rule_id.to_string())
                .or_insert_with(|| AtomicUsize::new(0));

            let next_index = (current_index + 1) % response_count;
            index.store(next_index, Ordering::SeqCst);

            current_index % response_count
        } else {
            current_index % response_count
        }
    }

    /// Reset cycling state for a rule
    #[allow(dead_code)]
    pub fn reset(&self, rule_id: &str) {
        if let Some(index) = self.indices.write().get(rule_id) {
            index.store(0, Ordering::SeqCst);
        }
        if let Some(counter) = self.repeat_counters.write().get(rule_id) {
            counter.store(0, Ordering::SeqCst);
        }
    }

    /// Reset all cycling state
    #[allow(dead_code)]
    pub fn reset_all(&self) {
        self.indices.write().clear();
        self.repeat_counters.write().clear();
    }

    /// Peek at current response index without modifying state
    /// Used to check response type before committing to cycling
    pub fn peek_response_index(&self, rule_id: &str, response_count: usize) -> usize {
        if response_count == 0 {
            return 0;
        }

        let indices = self.indices.read();
        if let Some(index_entry) = indices.get(rule_id) {
            index_entry.load(Ordering::SeqCst) % response_count
        } else {
            0
        }
    }

    /// Advance the cycler for a proxy response (which has no repeat behavior)
    /// This should be called after successfully handling a proxy response
    pub fn advance_for_proxy(&self, rule_id: &str, response_count: usize) {
        if response_count == 0 {
            return;
        }

        let mut indices = self.indices.write();
        let index_entry = indices
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let current_index = index_entry.load(Ordering::SeqCst) % response_count;
        let next_index = (current_index + 1) % response_count;
        index_entry.store(next_index, Ordering::SeqCst);
    }

    /// Get response index with per-response repeat values
    /// Each response can have its own repeat count via _behaviors.repeat
    pub fn get_response_index_with_per_response_repeat<T: HasRepeatBehavior>(
        &self,
        rule_id: &str,
        responses: &[T],
    ) -> usize {
        if responses.is_empty() {
            return 0;
        }

        // Get current state
        let mut indices = self.indices.write();
        let mut counters = self.repeat_counters.write();

        let index_entry = indices
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));
        let counter_entry = counters
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let current_index = index_entry.load(Ordering::SeqCst) % responses.len();
        let current_repeat = counter_entry.load(Ordering::SeqCst);

        // Get repeat value for current response
        let repeat_count = responses[current_index].get_repeat().unwrap_or(1).max(1) as usize;

        // Increment repeat counter
        let new_repeat = current_repeat + 1;

        // Return current index and decide if we should advance
        if new_repeat >= repeat_count {
            // Reset repeat counter
            counter_entry.store(0, Ordering::SeqCst);
            // Advance to next response for next call
            let next_index = (current_index + 1) % responses.len();
            index_entry.store(next_index, Ordering::SeqCst);
        } else {
            // Increment repeat counter for next call
            counter_entry.store(new_repeat, Ordering::SeqCst);
        }

        current_index
    }
}

/// Trait for types that can have a repeat behavior
pub trait HasRepeatBehavior {
    fn get_repeat(&self) -> Option<u32>;
}

// =============================================================================
// COPY BEHAVIOR IMPLEMENTATION
// =============================================================================

/// Apply copy behaviors to response body
pub fn apply_copy_behaviors(
    body: &str,
    headers: &mut HashMap<String, String>,
    behaviors: &[CopyBehavior],
    request: &RequestContext,
) -> String {
    let mut result = body.to_string();

    for behavior in behaviors {
        // Extract value from request
        if let Some(source_value) = behavior.from.extract(request) {
            // Apply extraction method
            let extracted = behavior.extraction.extract(&source_value);
            let replacement = extracted.unwrap_or_default();

            // Replace token in body
            result = result.replace(&behavior.into, &replacement);

            // Also replace in headers
            for value in headers.values_mut() {
                *value = value.replace(&behavior.into, &replacement);
            }
        } else {
            // Source not found, replace with empty string
            result = result.replace(&behavior.into, "");
            for value in headers.values_mut() {
                *value = value.replace(&behavior.into, "");
            }
        }
    }

    result
}

// =============================================================================
// LOOKUP BEHAVIOR IMPLEMENTATION
// =============================================================================

/// CSV data cache for performance
pub struct CsvCache {
    data: RwLock<HashMap<String, Arc<CsvData>>>,
}

impl Default for CsvCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvCache {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Get or load CSV data
    pub fn get_or_load(&self, path: &str, delimiter: char) -> Option<Arc<CsvData>> {
        // Check cache first
        {
            let cache = self.data.read();
            if let Some(data) = cache.get(path) {
                return Some(Arc::clone(data));
            }
        }

        // Load from file
        let data = CsvData::load(path, delimiter).ok()?;
        let data = Arc::new(data);

        // Cache it
        {
            let mut cache = self.data.write();
            cache.insert(path.to_string(), Arc::clone(&data));
        }

        Some(data)
    }

    /// Clear cache
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.data.write().clear();
    }
}

/// Parsed CSV data
pub struct CsvData {
    /// Column headers
    headers: Vec<String>,
    /// Rows indexed by first column for fast lookup
    rows: HashMap<String, Vec<String>>,
}

impl CsvData {
    /// Load CSV from file
    pub fn load<P: AsRef<Path>>(path: P, delimiter: char) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // Parse header row
        let header_line = lines
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty CSV"))??;
        let headers: Vec<String> = header_line
            .split(delimiter)
            .map(|s| s.trim().to_string())
            .collect();

        // Parse data rows
        let mut rows = HashMap::new();
        for line in lines {
            let line = line?;
            let values: Vec<String> = line
                .split(delimiter)
                .map(|s| s.trim().to_string())
                .collect();
            if !values.is_empty() {
                rows.insert(values[0].clone(), values);
            }
        }

        Ok(Self { headers, rows })
    }

    /// Lookup a row by key and return column values as token replacements
    pub fn lookup(&self, key: &str, key_column: &str) -> HashMap<String, String> {
        let mut result = HashMap::new();

        // Find key column index
        let key_col_idx = self.headers.iter().position(|h| h == key_column);

        if let Some(key_idx) = key_col_idx {
            // Find row where key column matches
            for (row_key, values) in &self.rows {
                let matches = if key_idx == 0 {
                    row_key == key
                } else {
                    values.get(key_idx).map(|v| v == key).unwrap_or(false)
                };

                if matches {
                    // Return all columns as [column_name] tokens
                    for (i, header) in self.headers.iter().enumerate() {
                        if let Some(value) = values.get(i) {
                            result.insert(format!("[{header}]"), value.clone());
                        }
                    }
                    break;
                }
            }
        }

        result
    }
}

/// Apply lookup behaviors to response body
pub fn apply_lookup_behaviors(
    body: &str,
    headers: &mut HashMap<String, String>,
    behaviors: &[LookupBehavior],
    request: &RequestContext,
    csv_cache: &CsvCache,
) -> String {
    let mut result = body.to_string();

    for behavior in behaviors {
        // Extract key from request
        let key_value = behavior
            .key
            .from
            .extract(request)
            .and_then(|v| behavior.key.extraction.extract(&v));

        if let Some(key) = key_value {
            // Load CSV data
            if let Some(csv_data) = csv_cache.get_or_load(
                &behavior.from_data_source.csv.path,
                behavior.from_data_source.csv.delimiter,
            ) {
                // Lookup row
                let replacements = csv_data.lookup(&key, &behavior.from_data_source.csv.key_column);

                // Apply replacements
                for (token, value) in replacements {
                    let full_token = format!("{}{}", behavior.into, token);
                    result = result.replace(&full_token, &value);
                    for header_value in headers.values_mut() {
                        *header_value = header_value.replace(&full_token, &value);
                    }
                }
            }
        }
    }

    result
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Extract value using JSONPath
/// Used by copy behaviors and predicate jsonpath parameter
pub fn extract_jsonpath(json_str: &str, path: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Simple JSONPath implementation (supports basic paths like $.field, $.array[0])
    let path = path.trim_start_matches('$').trim_start_matches('.');

    let mut current = &json;
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }

        // Check for array index
        if let Some(bracket_pos) = part.find('[') {
            let field = &part[..bracket_pos];
            let index_str = &part[bracket_pos + 1..part.len() - 1];

            if !field.is_empty() {
                current = current.get(field)?;
            }

            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(part)?;
        }
    }

    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null => Some("null".to_string()),
        _ => Some(current.to_string()),
    }
}

/// Extract value using XPath
/// Used by copy behaviors and predicate xpath parameter
pub fn extract_xpath(xml_str: &str, path: &str) -> Option<String> {
    use sxd_document::parser;
    use sxd_xpath::{evaluate_xpath, Value};

    let package = parser::parse(xml_str).ok()?;
    let document = package.as_document();

    match evaluate_xpath(&document, path) {
        Ok(Value::String(s)) => Some(s),
        Ok(Value::Number(n)) => Some(n.to_string()),
        Ok(Value::Boolean(b)) => Some(b.to_string()),
        Ok(Value::Nodeset(nodes)) => nodes.iter().next().map(|n| n.string_value()),
        _ => None,
    }
}

// =============================================================================
// SHELL TRANSFORM IMPLEMENTATION
// =============================================================================

/// Execute shell transform command
/// The command receives MB_REQUEST and MB_RESPONSE environment variables
/// and should output the transformed response body to stdout
pub fn apply_shell_transform(
    command: &str,
    request: &RequestContext,
    response_body: &str,
    response_status: u16,
) -> Result<String, std::io::Error> {
    use std::process::Command;

    // Serialize request to JSON for MB_REQUEST
    let request_json = serde_json::json!({
        "method": request.method,
        "path": request.path,
        "query": request.query,
        "headers": request.headers,
        "body": request.body,
    });

    // Serialize response to JSON for MB_RESPONSE
    let response_json = serde_json::json!({
        "statusCode": response_status,
        "body": response_body,
    });

    // Execute command with environment variables
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("MB_REQUEST", request_json.to_string())
        .env("MB_RESPONSE", response_json.to_string())
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Shell transform failed: {stderr}"),
        ))
    }
}

/// Apply decorate behavior using Rhai script (Mountebank-compatible)
/// The script can access and modify `request` and `response` variables
pub fn apply_decorate(
    script: &str,
    request: &RequestContext,
    response_body: &str,
    response_status: u16,
    response_headers: &mut HashMap<String, String>,
) -> Result<(String, u16), String> {
    use rhai::{Dynamic, Engine, Map, Scope};

    let engine = Engine::new();
    let mut scope = Scope::new();

    // Create request map for Rhai
    let mut req_map = Map::new();
    req_map.insert("method".into(), Dynamic::from(request.method.clone()));
    req_map.insert("path".into(), Dynamic::from(request.path.clone()));
    req_map.insert(
        "body".into(),
        Dynamic::from(request.body.clone().unwrap_or_default()),
    );

    let mut query_map = Map::new();
    for (k, v) in &request.query {
        query_map.insert(k.clone().into(), Dynamic::from(v.clone()));
    }
    req_map.insert("query".into(), Dynamic::from(query_map));

    let mut headers_map = Map::new();
    for (k, v) in &request.headers {
        headers_map.insert(k.clone().into(), Dynamic::from(v.clone()));
    }
    req_map.insert("headers".into(), Dynamic::from(headers_map));

    // Create response map for Rhai
    let mut resp_map = Map::new();
    resp_map.insert("statusCode".into(), Dynamic::from(response_status as i64));
    resp_map.insert("body".into(), Dynamic::from(response_body.to_string()));

    let mut resp_headers_map = Map::new();
    for (k, v) in response_headers.iter() {
        resp_headers_map.insert(k.clone().into(), Dynamic::from(v.clone()));
    }
    resp_map.insert("headers".into(), Dynamic::from(resp_headers_map));

    scope.push("request", req_map);
    scope.push("response", resp_map);

    // Execute the decoration script
    match engine.eval_with_scope::<Dynamic>(&mut scope, script) {
        Ok(_) => {
            // Extract modified response from scope
            if let Some(response) = scope.get_value::<Map>("response") {
                let new_body = response
                    .get("body")
                    .and_then(|v| v.clone().try_cast::<String>())
                    .unwrap_or_else(|| response_body.to_string());

                let new_status = response
                    .get("statusCode")
                    .and_then(|v| v.clone().try_cast::<i64>())
                    .map(|s| s as u16)
                    .unwrap_or(response_status);

                // Update headers from response map
                if let Some(headers) = response.get("headers") {
                    if let Some(headers_map) = headers.clone().try_cast::<Map>() {
                        for (k, v) in headers_map {
                            if let Some(value) = v.try_cast::<String>() {
                                response_headers.insert(k.to_string(), value);
                            }
                        }
                    }
                }

                Ok((new_body, new_status))
            } else {
                Ok((response_body.to_string(), response_status))
            }
        }
        Err(e) => Err(format!("Decorate script error: {e}")),
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_behavior_fixed() {
        let wait = WaitBehavior::Fixed(100);
        assert_eq!(wait.get_duration_ms(), 100);
    }

    #[test]
    fn test_wait_behavior_range() {
        let wait = WaitBehavior::Range {
            min_ms: 100,
            max_ms: 200,
        };
        for _ in 0..10 {
            let duration = wait.get_duration_ms();
            assert!((100..=200).contains(&duration));
        }
    }

    #[test]
    fn test_response_cycler_basic() {
        let cycler = ResponseCycler::new();

        // With 3 responses, no repeat
        assert_eq!(cycler.get_response_index("rule1", 3, None), 0);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 1);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 2);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 0); // Wrap around
    }

    #[test]
    fn test_response_cycler_with_repeat() {
        let cycler = ResponseCycler::new();

        // With 2 responses, repeat=3
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1); // Advance after 3 repeats
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0); // Wrap around
    }

    #[test]
    fn test_copy_source_simple() {
        let request = RequestContext {
            method: "GET".to_string(),
            path: "/users/123".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Some("test body".to_string()),
        };

        let source = CopySource::Simple("path".to_string());
        assert_eq!(source.extract(&request), Some("/users/123".to_string()));

        let source = CopySource::Simple("method".to_string());
        assert_eq!(source.extract(&request), Some("GET".to_string()));

        let source = CopySource::Simple("body".to_string());
        assert_eq!(source.extract(&request), Some("test body".to_string()));
    }

    #[test]
    fn test_copy_source_nested() {
        let mut query = HashMap::new();
        query.insert("name".to_string(), "Alice".to_string());

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/test".to_string(),
            query,
            headers,
            body: None,
        };

        let mut map = HashMap::new();
        map.insert("query".to_string(), "name".to_string());
        let source = CopySource::Nested(map);
        assert_eq!(source.extract(&request), Some("Alice".to_string()));

        let mut map = HashMap::new();
        map.insert("headers".to_string(), "Content-Type".to_string());
        let source = CopySource::Nested(map);
        assert_eq!(
            source.extract(&request),
            Some("application/json".to_string())
        );
    }

    #[test]
    fn test_extraction_regex() {
        let method = ExtractionMethod::Regex {
            selector: r"/users/(\d+)".to_string(),
        };
        assert_eq!(method.extract("/users/123"), Some("123".to_string()));
        assert_eq!(method.extract("/posts/456"), None);
    }

    #[test]
    fn test_extraction_regex_full_match() {
        let method = ExtractionMethod::Regex {
            selector: r".*".to_string(),
        };
        assert_eq!(
            method.extract("hello world"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn test_extraction_jsonpath() {
        let method = ExtractionMethod::JsonPath {
            selector: "$.user.name".to_string(),
        };
        let json = r#"{"user": {"name": "Alice", "age": 30}}"#;
        assert_eq!(method.extract(json), Some("Alice".to_string()));
    }

    #[test]
    fn test_extraction_jsonpath_array() {
        let method = ExtractionMethod::JsonPath {
            selector: "$.items[0]".to_string(),
        };
        let json = r#"{"items": ["first", "second"]}"#;
        assert_eq!(method.extract(json), Some("first".to_string()));
    }

    #[test]
    fn test_apply_copy_behaviors() {
        let mut query = HashMap::new();
        query.insert("name".to_string(), "Alice".to_string());

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/users/123".to_string(),
            query,
            headers: HashMap::new(),
            body: None,
        };

        let behaviors = vec![
            CopyBehavior {
                from: CopySource::Simple("path".to_string()),
                into: "${PATH}".to_string(),
                extraction: ExtractionMethod::Regex {
                    selector: r"/users/(\d+)".to_string(),
                },
            },
            CopyBehavior {
                from: {
                    let mut map = HashMap::new();
                    map.insert("query".to_string(), "name".to_string());
                    CopySource::Nested(map)
                },
                into: "${NAME}".to_string(),
                extraction: ExtractionMethod::Regex {
                    selector: ".*".to_string(),
                },
            },
        ];

        let body = r#"{"userId": "${PATH}", "greeting": "Hello, ${NAME}!"}"#;
        let mut headers = HashMap::new();

        let result = apply_copy_behaviors(body, &mut headers, &behaviors, &request);
        assert_eq!(result, r#"{"userId": "123", "greeting": "Hello, Alice!"}"#);
    }

    #[test]
    fn test_wait_behavior_serde() {
        let yaml = "100";
        let wait: WaitBehavior = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(wait, WaitBehavior::Fixed(100)));

        let yaml = "min: 100\nmax: 200";
        let wait: WaitBehavior = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(
            wait,
            WaitBehavior::Range {
                min_ms: 100,
                max_ms: 200
            }
        ));
    }

    #[test]
    fn test_response_behaviors_serde() {
        let yaml = r#"
wait: 500
repeat: 3
copy:
  - from: path
    into: "${PATH}"
    using:
      method: regex
      selector: ".*"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(500))));
        assert_eq!(behaviors.repeat, Some(3));
        assert_eq!(behaviors.copy.len(), 1);
    }

    #[test]
    fn test_shell_transform_config_serde() {
        let yaml = r#"
wait: 100
shellTransform: "echo 'transformed'"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(100))));
        assert_eq!(
            behaviors.shell_transform,
            Some("echo 'transformed'".to_string())
        );
    }

    #[test]
    fn test_apply_shell_transform_echo() {
        // Test that shell_transform executes a simple echo command
        use super::apply_shell_transform;

        let request = RequestContext {
            method: "POST".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Some(r#"{"test": "data"}"#.to_string()),
        };

        // Simple echo command that outputs a fixed string
        let result = apply_shell_transform("echo 'hello world'", &request, "original body", 200);
        assert!(result.is_ok(), "Shell transform should succeed");
        assert!(
            result.unwrap().contains("hello world"),
            "Shell transform should output echo result"
        );
    }

    #[test]
    fn test_apply_shell_transform_with_env_vars() {
        // Test that MB_REQUEST and MB_RESPONSE env vars are available
        use super::apply_shell_transform;

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/users/123".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        // Command that outputs the MB_REQUEST env var (which contains JSON)
        let result = apply_shell_transform("echo $MB_REQUEST", &request, "test body", 200);
        assert!(result.is_ok(), "Shell transform should succeed");

        let output = result.unwrap();
        // The output should contain parts of the request context
        assert!(
            output.contains("GET") || output.contains("method"),
            "MB_REQUEST should contain request method"
        );
    }

    #[test]
    fn test_apply_decorate_modify_body() {
        use super::apply_decorate;

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        // Script that modifies the response body
        let script = r#"
            response.body = "modified body";
        "#;

        let result = apply_decorate(script, &request, "original body", 200, &mut headers);
        assert!(result.is_ok());
        let (body, status) = result.unwrap();
        assert_eq!(body, "modified body");
        assert_eq!(status, 200);
    }

    #[test]
    fn test_apply_decorate_modify_status() {
        use super::apply_decorate;

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        let mut headers = HashMap::new();

        // Script that modifies the response status
        let script = r#"
            response.statusCode = 201;
        "#;

        let result = apply_decorate(script, &request, "body", 200, &mut headers);
        assert!(result.is_ok());
        let (body, status) = result.unwrap();
        assert_eq!(body, "body");
        assert_eq!(status, 201);
    }

    #[test]
    fn test_apply_decorate_access_request() {
        use super::apply_decorate;

        let request = RequestContext {
            method: "POST".to_string(),
            path: "/users".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Some(r#"{"name": "Alice"}"#.to_string()),
        };

        let mut headers = HashMap::new();

        // Script that uses request data in response
        let script = r#"
            response.body = "Method: " + request.method + ", Path: " + request.path;
        "#;

        let result = apply_decorate(script, &request, "original", 200, &mut headers);
        assert!(result.is_ok());
        let (body, _status) = result.unwrap();
        assert!(body.contains("Method: POST"));
        assert!(body.contains("Path: /users"));
    }

    #[test]
    fn test_apply_decorate_modify_headers() {
        use super::apply_decorate;

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());

        // Script that modifies response headers
        let script = r#"
            response.headers["x-custom"] = "custom-value";
        "#;

        let result = apply_decorate(script, &request, "body", 200, &mut headers);
        assert!(result.is_ok());
        assert_eq!(headers.get("x-custom"), Some(&"custom-value".to_string()));
    }

    #[test]
    fn test_apply_decorate_script_error() {
        use super::apply_decorate;

        let request = RequestContext {
            method: "GET".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        let mut headers = HashMap::new();

        // Invalid script with syntax error
        let script = "this is not valid rhai {{{";

        let result = apply_decorate(script, &request, "body", 200, &mut headers);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Decorate script error"));
    }

    #[test]
    fn test_decorate_behavior_serde() {
        let yaml = r#"
wait: 100
decorate: "response.body = 'decorated';"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(100))));
        assert_eq!(
            behaviors.decorate,
            Some("response.body = 'decorated';".to_string())
        );
    }
}

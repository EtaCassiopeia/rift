//! Application state and logic for the TUI

use crate::api::{
    ApiClient, CreateImposterRequest, ImposterDetail, ImposterSummary, MetricsData, Stub,
};
use crate::components::{EditorAction, TextEditor};
use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Maximum number of metrics snapshots to keep for sparklines
const MAX_METRICS_HISTORY: usize = 60;

/// Current view/screen
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    ImposterList,
    ImposterDetail { port: u16 },
    StubDetail { port: u16, index: usize },
    StubEdit { port: u16, index: Option<usize> },
    Metrics,
}

/// Overlay (modal) state
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    Help,
    Confirm {
        message: String,
        action: PendingAction,
    },
    Error {
        message: String,
    },
    Input {
        prompt: String,
        action: InputAction,
    },
    Export {
        title: String,
        content: String,
        port: Option<u16>, // For save/apply operations
    },
    FilePathInput {
        prompt: String,
        action: FileAction,
    },
    Success {
        message: String,
    },
}

/// File-related actions
#[derive(Debug, Clone, PartialEq)]
pub enum FileAction {
    SaveExport { content: String, port: u16 },
    ImportFile,
    ImportFolder,
    ExportAll,
    ExportToFolder,
}

/// Actions that need confirmation
#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    DeleteImposter { port: u16 },
    DeleteStub { port: u16, index: usize },
    ClearRequests { port: u16 },
    ClearProxyResponses { port: u16 },
    ApplyRecordedStubs { port: u16 },
}

/// Input actions
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    CreateImposter,
    CreateProxyImposter,
}

/// Status message level
#[derive(Debug, Clone, PartialEq)]
pub enum StatusLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Metrics history snapshot
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub timestamp: Instant,
    pub total_requests: u64,
    pub per_imposter: HashMap<u16, u64>,
}

/// Parts of a curl request extracted from stub predicates
#[derive(Debug)]
struct CurlRequestParts {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    query_params: Vec<(String, String)>,
    json_body_parts: Vec<(String, serde_json::Value)>,
    raw_body: Option<String>,
}

impl Default for CurlRequestParts {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers: Vec::new(),
            query_params: Vec::new(),
            json_body_parts: Vec::new(),
            raw_body: None,
        }
    }
}

/// Focus area for split views
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusArea {
    Left,
    Right,
}

/// Editor state for stub editing
pub struct StubEditor {
    pub editor: TextEditor,
    pub validation_error: Option<String>,
    pub original_json: String,
}

impl StubEditor {
    pub fn new(json: &str) -> Self {
        let formatted = if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| json.to_string())
        } else {
            json.to_string()
        };

        let editor = TextEditor::new(&formatted);

        Self {
            editor,
            validation_error: None,
            original_json: json.to_string(),
        }
    }

    /// Validate the JSON content
    pub fn validate(&mut self) -> bool {
        let content = self.editor.content();
        match serde_json::from_str::<Stub>(&content) {
            Ok(_) => {
                self.validation_error = None;
                true
            }
            Err(e) => {
                self.validation_error = Some(format!("Invalid JSON: {}", e));
                false
            }
        }
    }

    /// Get the stub if valid
    pub fn get_stub(&self) -> Option<Stub> {
        let content = self.editor.content();
        serde_json::from_str(&content).ok()
    }

    /// Format the JSON content
    pub fn format(&mut self) {
        let content = self.editor.content();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Ok(formatted) = serde_json::to_string_pretty(&v) {
                self.editor.set_content(&formatted);
                self.validation_error = None;
            }
        }
    }

    /// Handle key input
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<EditorAction> {
        self.editor.handle_key(key)
    }
}

/// Input state for dialogs
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub port: String,
    pub name: String,
    pub protocol: String,
    pub target_url: String,
    pub proxy_mode: usize, // 0=proxyOnce, 1=proxyAlways, 2=proxyTransparent
    pub focus_field: usize,
    pub file_path: String,
    pub cursor_pos: usize, // Cursor position in file_path
}

impl InputState {
    pub fn proxy_mode_str(&self) -> &str {
        match self.proxy_mode {
            0 => "proxyOnce",
            1 => "proxyAlways",
            2 => "proxyTransparent",
            _ => "proxyOnce",
        }
    }

    pub fn proxy_mode_display(&self) -> &str {
        match self.proxy_mode {
            0 => "proxyOnce (record first, replay after)",
            1 => "proxyAlways (always forward, keep recording)",
            2 => "proxyTransparent (always forward, no recording)",
            _ => "proxyOnce",
        }
    }
}

/// Main application state
pub struct App {
    // Navigation
    pub view: View,
    pub view_stack: Vec<View>,
    pub overlay: Overlay,

    // Data
    pub imposters: Vec<ImposterSummary>,
    pub current_imposter: Option<ImposterDetail>,
    pub metrics: MetricsData,
    pub metrics_history: VecDeque<MetricsSnapshot>,

    // UI State
    pub imposter_list_state: ListState,
    pub stub_list_state: ListState,
    pub request_list_state: ListState,
    pub focus: FocusArea,
    pub status_message: Option<(String, StatusLevel, Instant)>,

    // Search State
    pub search_active: bool,
    pub search_query: String,

    // Edit State
    pub stub_editor: Option<StubEditor>,
    pub input_state: InputState,
    pub export_scroll_offset: u16,
    pub help_scroll: u16,
    pub help_max_scroll: u16,

    // Connection
    pub client: ApiClient,
    pub admin_url: String,
    pub theme: Theme,

    // Runtime
    pub should_quit: bool,
    pub is_loading: bool,
    pub is_connected: bool,
    pub last_refresh: Instant,
    pub start_time: Instant,
    pub refresh_interval: Duration,
}

impl App {
    /// Create a new App instance
    pub async fn new(admin_url: &str, refresh_interval: Duration) -> Self {
        let client = ApiClient::new(admin_url);

        let mut app = Self {
            view: View::ImposterList,
            view_stack: Vec::new(),
            overlay: Overlay::None,

            imposters: Vec::new(),
            current_imposter: None,
            metrics: MetricsData::default(),
            metrics_history: VecDeque::with_capacity(MAX_METRICS_HISTORY),

            imposter_list_state: ListState::default(),
            stub_list_state: ListState::default(),
            request_list_state: ListState::default(),
            focus: FocusArea::Left,
            status_message: None,

            search_active: false,
            search_query: String::new(),

            stub_editor: None,
            input_state: InputState {
                protocol: "http".to_string(),
                ..Default::default()
            },
            export_scroll_offset: 0,
            help_scroll: 0,
            help_max_scroll: 0,

            client,
            admin_url: admin_url.to_string(),
            theme: Theme::default(),

            should_quit: false,
            is_loading: false,
            is_connected: false,
            last_refresh: Instant::now(),
            start_time: Instant::now(),
            refresh_interval,
        };

        // Initial data load
        app.refresh().await;
        app
    }

    /// Refresh all data from the API
    pub async fn refresh(&mut self) {
        self.is_loading = true;

        // Check connection
        match self.client.health_check().await {
            Ok(healthy) => {
                self.is_connected = healthy;
            }
            Err(_) => {
                self.is_connected = false;
                self.is_loading = false;
                return;
            }
        }

        // Load imposters
        match self.client.list_imposters().await {
            Ok(imposters) => {
                self.imposters = imposters;
                // Ensure selection is valid
                if !self.imposters.is_empty() {
                    if self.imposter_list_state.selected().is_none() {
                        self.imposter_list_state.select(Some(0));
                    } else if let Some(idx) = self.imposter_list_state.selected() {
                        if idx >= self.imposters.len() {
                            self.imposter_list_state
                                .select(Some(self.imposters.len() - 1));
                        }
                    }
                }
            }
            Err(e) => {
                self.set_status(
                    format!("Failed to load imposters: {}", e),
                    StatusLevel::Error,
                );
            }
        }

        // Load metrics
        if let Ok(metrics) = self.client.get_metrics().await {
            // Update history
            let snapshot = MetricsSnapshot {
                timestamp: Instant::now(),
                total_requests: metrics.total_requests,
                per_imposter: metrics
                    .per_imposter
                    .iter()
                    .map(|(k, v)| (*k, v.request_count))
                    .collect(),
            };
            self.metrics_history.push_back(snapshot);
            if self.metrics_history.len() > MAX_METRICS_HISTORY {
                self.metrics_history.pop_front();
            }

            self.metrics = metrics;
        }

        // Refresh current imposter if viewing detail
        if let View::ImposterDetail { port } | View::StubDetail { port, .. } = self.view {
            if let Ok(detail) = self.client.get_imposter(port).await {
                self.current_imposter = Some(detail);
            }
        }

        self.is_loading = false;
        self.last_refresh = Instant::now();
    }

    /// Set a status message
    pub fn set_status(&mut self, message: String, level: StatusLevel) {
        self.status_message = Some((message, level, Instant::now()));
    }

    /// Clear status if expired
    pub fn clear_expired_status(&mut self) {
        if let Some((_, _, time)) = &self.status_message {
            if time.elapsed() > Duration::from_secs(5) {
                self.status_message = None;
            }
        }
    }

    /// Cycle to the next theme
    pub fn cycle_theme(&mut self) {
        self.theme.next();
        self.set_status(
            format!("Theme: {}", self.theme.preset.name()),
            StatusLevel::Info,
        );
    }

    /// Navigate to a new view
    pub fn navigate(&mut self, view: View) {
        self.view_stack.push(self.view.clone());
        self.view = view;
        // Clear search when navigating
        self.search_active = false;
        self.search_query.clear();
    }

    /// Go back to previous view
    pub fn go_back(&mut self) {
        // Clear search when going back
        if self.search_active || !self.search_query.is_empty() {
            self.search_active = false;
            self.search_query.clear();
            return;
        }

        if let Some(prev) = self.view_stack.pop() {
            self.view = prev;
        } else if self.view != View::ImposterList {
            self.view = View::ImposterList;
        } else {
            self.should_quit = true;
        }
    }

    /// Move selection up in current list (skips filtered items when search is active)
    pub fn select_previous(&mut self) {
        match &self.view {
            View::ImposterList => {
                if self.imposters.is_empty() {
                    return;
                }

                // Get indices of matching imposters
                let matching_indices: Vec<usize> = if self.search_query.is_empty() {
                    (0..self.imposters.len()).collect()
                } else {
                    self.imposters
                        .iter()
                        .enumerate()
                        .filter(|(_, imp)| self.imposter_matches_search(imp))
                        .map(|(i, _)| i)
                        .collect()
                };

                if matching_indices.is_empty() {
                    return;
                }

                let current = self.imposter_list_state.selected().unwrap_or(0);
                // Find the previous matching index
                let next = matching_indices
                    .iter()
                    .rev()
                    .find(|&&i| i < current)
                    .or_else(|| matching_indices.last())
                    .copied()
                    .unwrap_or(current);

                self.imposter_list_state.select(Some(next));
            }
            View::ImposterDetail { .. } | View::StubDetail { .. } => {
                if self.focus == FocusArea::Left {
                    if let Some(imp) = &self.current_imposter {
                        if imp.stubs.is_empty() {
                            return;
                        }

                        // Get indices of matching stubs
                        let matching_indices = self.filtered_stubs();

                        if matching_indices.is_empty() {
                            return;
                        }

                        let current = self.stub_list_state.selected().unwrap_or(0);
                        let next = matching_indices
                            .iter()
                            .rev()
                            .find(|&&i| i < current)
                            .or_else(|| matching_indices.last())
                            .copied()
                            .unwrap_or(current);

                        self.stub_list_state.select(Some(next));
                    }
                } else if let Some(imp) = &self.current_imposter {
                    let len = imp.requests.len();
                    if len > 0 {
                        let i = self
                            .request_list_state
                            .selected()
                            .map(|i| if i == 0 { len - 1 } else { i - 1 })
                            .unwrap_or(0);
                        self.request_list_state.select(Some(i));
                    }
                }
            }
            _ => {}
        }
    }

    /// Move selection down in current list (skips filtered items when search is active)
    pub fn select_next(&mut self) {
        match &self.view {
            View::ImposterList => {
                if self.imposters.is_empty() {
                    return;
                }

                // Get indices of matching imposters
                let matching_indices: Vec<usize> = if self.search_query.is_empty() {
                    (0..self.imposters.len()).collect()
                } else {
                    self.imposters
                        .iter()
                        .enumerate()
                        .filter(|(_, imp)| self.imposter_matches_search(imp))
                        .map(|(i, _)| i)
                        .collect()
                };

                if matching_indices.is_empty() {
                    return;
                }

                let current = self.imposter_list_state.selected().unwrap_or(0);
                // Find the next matching index
                let next = matching_indices
                    .iter()
                    .find(|&&i| i > current)
                    .or_else(|| matching_indices.first())
                    .copied()
                    .unwrap_or(current);

                self.imposter_list_state.select(Some(next));
            }
            View::ImposterDetail { .. } | View::StubDetail { .. } => {
                if self.focus == FocusArea::Left {
                    if let Some(imp) = &self.current_imposter {
                        if imp.stubs.is_empty() {
                            return;
                        }

                        // Get indices of matching stubs
                        let matching_indices = self.filtered_stubs();

                        if matching_indices.is_empty() {
                            return;
                        }

                        let current = self.stub_list_state.selected().unwrap_or(0);
                        let next = matching_indices
                            .iter()
                            .find(|&&i| i > current)
                            .or_else(|| matching_indices.first())
                            .copied()
                            .unwrap_or(current);

                        self.stub_list_state.select(Some(next));
                    }
                } else if let Some(imp) = &self.current_imposter {
                    let len = imp.requests.len();
                    if len > 0 {
                        let i = self
                            .request_list_state
                            .selected()
                            .map(|i| (i + 1) % len)
                            .unwrap_or(0);
                        self.request_list_state.select(Some(i));
                    }
                }
            }
            _ => {}
        }
    }

    /// Get selected imposter
    pub fn selected_imposter(&self) -> Option<&ImposterSummary> {
        self.imposter_list_state
            .selected()
            .and_then(|i| self.imposters.get(i))
    }

    /// Enter imposter detail view
    pub async fn enter_imposter_detail(&mut self) {
        if let Some(imp) = self.selected_imposter() {
            let port = imp.port;
            self.is_loading = true;
            if let Ok(detail) = self.client.get_imposter(port).await {
                self.current_imposter = Some(detail);
                self.stub_list_state.select(Some(0));
                self.navigate(View::ImposterDetail { port });
            } else {
                self.set_status(
                    format!("Failed to load imposter :{}", port),
                    StatusLevel::Error,
                );
            }
            self.is_loading = false;
        }
    }

    /// Toggle enable/disable for selected imposter
    pub async fn toggle_imposter(&mut self) {
        let port = match &self.view {
            View::ImposterList => self.selected_imposter().map(|i| i.port),
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            // Find if enabled
            let enabled = self
                .imposters
                .iter()
                .find(|i| i.port == port)
                .map(|i| i.enabled);

            let result = if enabled.unwrap_or(true) {
                self.client.disable_imposter(port).await
            } else {
                self.client.enable_imposter(port).await
            };

            match result {
                Ok(_) => {
                    let action = if enabled.unwrap_or(true) {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    self.set_status(
                        format!("Imposter :{} {}", port, action),
                        StatusLevel::Success,
                    );
                    self.refresh().await;
                }
                Err(e) => {
                    self.set_status(
                        format!("Failed to toggle imposter: {}", e),
                        StatusLevel::Error,
                    );
                }
            }
        }
    }

    /// Show delete confirmation
    pub fn confirm_delete_imposter(&mut self) {
        if let Some(imp) = self.selected_imposter() {
            self.overlay = Overlay::Confirm {
                message: format!(
                    "Delete imposter :{}{}?",
                    imp.port,
                    imp.name
                        .as_ref()
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                ),
                action: PendingAction::DeleteImposter { port: imp.port },
            };
        }
    }

    /// Delete an imposter
    pub async fn delete_imposter(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.delete_imposter(port).await {
            Ok(_) => {
                self.set_status(format!("Deleted imposter :{}", port), StatusLevel::Success);
                self.refresh().await;
                if matches!(self.view, View::ImposterDetail { port: p } if p == port) {
                    self.view = View::ImposterList;
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to delete: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Show create imposter dialog
    pub fn show_create_imposter(&mut self) {
        self.input_state = InputState {
            port: String::new(),
            name: String::new(),
            protocol: "http".to_string(),
            target_url: String::new(),
            proxy_mode: 0,
            focus_field: 0,
            file_path: String::new(),
            cursor_pos: 0,
        };
        self.overlay = Overlay::Input {
            prompt: "Create New Imposter".to_string(),
            action: InputAction::CreateImposter,
        };
    }

    /// Create a new imposter
    pub async fn create_imposter(&mut self) {
        let port = if self.input_state.port.is_empty() {
            None
        } else {
            match self.input_state.port.parse::<u16>() {
                Ok(p) => Some(p),
                Err(_) => {
                    self.set_status("Invalid port number".to_string(), StatusLevel::Error);
                    return;
                }
            }
        };

        let request = CreateImposterRequest {
            port,
            protocol: self.input_state.protocol.clone(),
            name: if self.input_state.name.is_empty() {
                None
            } else {
                Some(self.input_state.name.clone())
            },
            record_requests: false,
            stubs: Vec::new(),
        };

        self.is_loading = true;
        match self.client.create_imposter(request).await {
            Ok(port) => {
                self.set_status(format!("Created imposter :{}", port), StatusLevel::Success);
                self.overlay = Overlay::None;
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to create: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }

    /// Show create proxy imposter dialog
    pub fn show_create_proxy_imposter(&mut self) {
        self.input_state = InputState {
            port: String::new(),
            name: String::new(),
            protocol: "http".to_string(),
            target_url: String::new(),
            proxy_mode: 0, // proxyOnce
            focus_field: 0,
            file_path: String::new(),
            cursor_pos: 0,
        };
        self.overlay = Overlay::Input {
            prompt: "Create Proxy Imposter".to_string(),
            action: InputAction::CreateProxyImposter,
        };
    }

    /// Create a proxy imposter for recording
    pub async fn create_proxy_imposter(&mut self) {
        if self.input_state.target_url.is_empty() {
            self.set_status("Target URL is required".to_string(), StatusLevel::Error);
            return;
        }

        let port = if self.input_state.port.is_empty() {
            None
        } else {
            match self.input_state.port.parse::<u16>() {
                Ok(p) => Some(p),
                Err(_) => {
                    self.set_status("Invalid port number".to_string(), StatusLevel::Error);
                    return;
                }
            }
        };

        let name = if self.input_state.name.is_empty() {
            None
        } else {
            Some(self.input_state.name.clone())
        };

        self.is_loading = true;
        match self
            .client
            .create_proxy_imposter(
                port,
                name,
                &self.input_state.target_url,
                self.input_state.proxy_mode_str(),
            )
            .await
        {
            Ok(port) => {
                self.set_status(
                    format!("Created proxy imposter :{}", port),
                    StatusLevel::Success,
                );
                self.overlay = Overlay::None;
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to create: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }

    /// Confirm clear proxy responses
    pub fn confirm_clear_proxy_responses(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            self.overlay = Overlay::Confirm {
                message: format!("Clear all proxy recordings for :{}?", port),
                action: PendingAction::ClearProxyResponses { port },
            };
        }
    }

    /// Clear proxy responses
    pub async fn clear_proxy_responses(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.clear_proxy_responses(port).await {
            Ok(_) => {
                self.set_status("Cleared proxy recordings".to_string(), StatusLevel::Success);
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to clear: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Export imposter config
    pub async fn export_imposter(&mut self, remove_proxies: bool) {
        let port = match &self.view {
            View::ImposterDetail { port } => *port,
            _ => return,
        };

        self.is_loading = true;
        match self.client.export_imposter(port, remove_proxies).await {
            Ok(json) => {
                let title = if remove_proxies {
                    format!(
                        "Exported Stubs (Port :{}) - [s]ave [c]opy [A]pply [Esc]close",
                        port
                    )
                } else {
                    format!(
                        "Exported Config (Port :{}) - [s]ave [c]opy [Esc]close",
                        port
                    )
                };
                self.overlay = Overlay::Export {
                    title,
                    content: json,
                    port: Some(port),
                };
            }
            Err(e) => {
                self.set_status(format!("Failed to export: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }

    /// Copy content to clipboard
    pub fn copy_to_clipboard(&mut self, content: &str) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(content.to_string()) {
                    self.set_status(format!("Failed to copy: {}", e), StatusLevel::Error);
                } else {
                    self.set_status("Copied to clipboard".to_string(), StatusLevel::Success);
                }
            }
            Err(e) => {
                self.set_status(
                    format!("Clipboard not available: {}", e),
                    StatusLevel::Error,
                );
            }
        }
    }

    /// Paste from clipboard, returning the text if successful
    fn paste_from_clipboard(&self) -> Option<String> {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
    }

    /// Generate a curl command for a stub
    pub fn generate_curl_command(&self, stub: &Stub, port: u16) -> String {
        let mut parts = CurlRequestParts::default();

        // Parse predicates to extract request info
        for predicate in &stub.predicates {
            self.extract_from_predicate(predicate, &mut parts);
        }

        let CurlRequestParts {
            method,
            path,
            headers,
            query_params,
            json_body_parts,
            raw_body,
        } = parts;

        // Build final body - combine jsonpath parts into one JSON object
        let body = if !json_body_parts.is_empty() {
            Some(self.merge_jsonpath_bodies(&json_body_parts))
        } else {
            raw_body
        };

        // Build the curl command
        let mut parts: Vec<String> = vec!["curl -s".to_string()];

        // Add method if not GET
        if method != "GET" {
            parts.push(format!("-X {}", method));
        }

        // Add Content-Type header if we have a body and it looks like JSON
        if body.is_some() {
            let has_content_type = headers
                .iter()
                .any(|(k, _)| k.to_lowercase() == "content-type");
            if !has_content_type {
                if let Some(ref b) = body {
                    if b.trim_start().starts_with('{') || b.trim_start().starts_with('[') {
                        parts.push("-H 'Content-Type: application/json'".to_string());
                    }
                }
            }
        }

        // Add headers
        for (key, value) in &headers {
            parts.push(format!("-H '{}: {}'", key, value));
        }

        // Add body if present
        if let Some(ref b) = body {
            parts.push(format!("-d '{}'", b.replace('\'', "'\\''")));
        }

        // Build URL with query params
        let mut url = format!("http://localhost:{}{}", port, path);
        if !query_params.is_empty() {
            let query_string: Vec<String> = query_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            url = format!("{}?{}", url, query_string.join("&"));
        }

        parts.push(format!("'{}'", url));

        parts.join(" \\\n  ")
    }

    /// Extract request info from a predicate
    fn extract_from_predicate(&self, predicate: &serde_json::Value, parts: &mut CurlRequestParts) {
        if let Some(obj) = predicate.as_object() {
            // Check for jsonpath - if present, we need to build a JSON body
            let jsonpath_selector = obj
                .get("jsonpath")
                .and_then(|jp| jp.as_object())
                .and_then(|jp| jp.get("selector"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());

            // Handle different predicate types: equals, contains, startsWith, deepEquals, matches, etc.
            for (pred_type, pred_value) in obj {
                if pred_type == "and" || pred_type == "or" {
                    // Handle composite predicates
                    if let Some(arr) = pred_value.as_array() {
                        for sub_pred in arr {
                            self.extract_from_predicate(sub_pred, parts);
                        }
                    }
                    continue;
                }

                // Skip non-predicate fields
                if pred_type == "jsonpath" || pred_type == "caseSensitive" || pred_type == "except"
                {
                    continue;
                }

                if let Some(inner) = pred_value.as_object() {
                    // Extract method
                    if let Some(m) = inner.get("method").and_then(|v| v.as_str()) {
                        parts.method = m.to_uppercase();
                    }

                    // Extract path - handle equals, deepEquals, contains, matches
                    if let Some(p) = inner.get("path").and_then(|v| v.as_str()) {
                        let extracted_path = match pred_type.as_str() {
                            "matches" => {
                                // Convert regex pattern to a sample path
                                self.regex_to_sample_path(p)
                            }
                            "contains" | "startsWith" | "endsWith" => {
                                // Use the partial path, ensuring it starts with /
                                if p.starts_with('/') {
                                    p.to_string()
                                } else {
                                    format!("/{}", p)
                                }
                            }
                            _ => {
                                // equals, deepEquals - use exact path
                                if p.starts_with('/') {
                                    p.to_string()
                                } else {
                                    format!("/{}", p)
                                }
                            }
                        };
                        // Only update if we have a more specific path
                        if parts.path == "/" || extracted_path.len() > parts.path.len() {
                            parts.path = extracted_path;
                        }
                    }

                    // Extract headers
                    if let Some(hdrs) = inner.get("headers").and_then(|v| v.as_object()) {
                        for (k, v) in hdrs {
                            if let Some(val) = v.as_str() {
                                parts.headers.push((k.clone(), val.to_string()));
                            }
                        }
                    }

                    // Extract query parameters
                    if let Some(q) = inner.get("query").and_then(|v| v.as_object()) {
                        for (k, v) in q {
                            if let Some(val) = v.as_str() {
                                parts.query_params.push((k.clone(), val.to_string()));
                            }
                        }
                    }

                    // Extract body - handle jsonpath case
                    if let Some(b) = inner.get("body") {
                        if let Some(ref selector) = jsonpath_selector {
                            // Collect jsonpath body parts to merge later
                            parts.json_body_parts.push((selector.clone(), b.clone()));
                        } else if let Some(s) = b.as_str() {
                            // Plain string body
                            parts.raw_body = Some(s.to_string());
                        } else {
                            // Already a JSON object
                            parts.raw_body = Some(serde_json::to_string(b).unwrap_or_default());
                        }
                    }
                }
            }
        }
    }

    /// Convert a regex pattern to a sample path
    /// e.g., "^/auto/dealers/[^/]+/dealer-customers/[^/]+$" -> "/auto/dealers/{1}/dealer-customers/{2}"
    fn regex_to_sample_path(&self, pattern: &str) -> String {
        let mut path = pattern.to_string();

        // Remove regex anchors
        path = path
            .trim_start_matches('^')
            .trim_end_matches('$')
            .to_string();

        // Replace [^/]+ patterns with numbered placeholders
        let mut counter = 1;
        while path.contains("[^/]+") {
            path = path.replacen("[^/]+", &format!("{{{}}}", counter), 1);
            counter += 1;
        }

        // Replace other common regex patterns
        path = path.replace(r"\d+", "123");
        path = path.replace(".+", "sample");
        path = path.replace(".*", "");
        path = path.replace(r"\.", ".");
        path = path.replace(r"\/", "/");
        path = path.replace("(?:", "");
        path = path.replace(")?", "");
        path = path.replace("(", "");
        path = path.replace(")", "");

        if !path.starts_with('/') {
            path = format!("/{}", path);
        }

        path
    }

    /// Merge multiple jsonpath body parts into a single JSON object
    /// e.g., [("$.user.id", "123"), ("$.user.name", "john")] -> {"user": {"id": "123", "name": "john"}}
    fn merge_jsonpath_bodies(&self, parts: &[(String, serde_json::Value)]) -> String {
        let mut root = serde_json::Map::new();

        for (selector, value) in parts {
            self.set_jsonpath_value(&mut root, selector, value.clone());
        }

        serde_json::to_string(&serde_json::Value::Object(root)).unwrap_or_else(|_| "{}".to_string())
    }

    /// Set a value at a jsonpath location in a JSON object
    /// Handles array notation like [:0] by wrapping values in arrays
    /// e.g., $.receiver.context.correlationKeys.[:0].keyValue with "728839"
    ///       -> {"receiver":{"context":{"correlationKeys":[{"keyValue":"728839"}]}}}
    fn set_jsonpath_value(
        &self,
        root: &mut serde_json::Map<String, serde_json::Value>,
        selector: &str,
        value: serde_json::Value,
    ) {
        let path = selector.trim_start_matches('$').trim_start_matches('.');
        if path.is_empty() {
            return;
        }

        // Parse parts - each part can have array notation like "correlationKeys[:0]"
        // The part BEFORE the array index should become an array
        let raw_parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();

        // Build structure from leaf to root
        let mut current = value;

        for i in (0..raw_parts.len()).rev() {
            let part = raw_parts[i];

            // Check if this part has array notation (means we're inside an array)
            if part.starts_with("[:") || part.starts_with("[") {
                // This is an array index - wrap in array
                current = serde_json::json!([current]);
                continue;
            }

            // Check if part has embedded array notation like "correlationKeys[:0]"
            let (field_name, has_array) = if let Some(bracket_pos) = part.find('[') {
                (&part[..bracket_pos], true)
            } else {
                (part, false)
            };

            if field_name.is_empty() {
                continue;
            }

            // Wrap current value in an object with this field name
            let mut obj = serde_json::Map::new();
            if has_array {
                obj.insert(field_name.to_string(), serde_json::json!([current]));
            } else {
                obj.insert(field_name.to_string(), current);
            }
            current = serde_json::Value::Object(obj);
        }

        // Merge the built structure into root
        if let serde_json::Value::Object(built) = current {
            self.deep_merge(root, built);
        }
    }

    /// Deep merge two JSON objects
    fn deep_merge(
        &self,
        target: &mut serde_json::Map<String, serde_json::Value>,
        source: serde_json::Map<String, serde_json::Value>,
    ) {
        for (key, value) in source {
            match (target.get_mut(&key), value) {
                (Some(serde_json::Value::Object(t)), serde_json::Value::Object(s)) => {
                    self.deep_merge(t, s);
                }
                (Some(serde_json::Value::Array(t)), serde_json::Value::Array(s)) => {
                    // Merge array contents - for arrays of objects, merge first elements
                    if let Some(serde_json::Value::Object(t_obj)) = t.first_mut() {
                        if let Some(serde_json::Value::Object(s_obj)) = s.into_iter().next() {
                            self.deep_merge(t_obj, s_obj);
                        }
                    }
                }
                (_, v) => {
                    target.insert(key, v);
                }
            }
        }
    }

    /// Copy curl command for selected stub to clipboard
    pub fn copy_stub_as_curl(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => *port,
            View::StubDetail { port, .. } => *port,
            _ => return,
        };

        let stub_index = match &self.view {
            View::StubDetail { index, .. } => Some(*index),
            View::ImposterDetail { .. } => self.stub_list_state.selected(),
            _ => None,
        };

        if let Some(idx) = stub_index {
            if let Some(imp) = &self.current_imposter {
                if let Some(stub) = imp.stubs.get(idx) {
                    let curl_cmd = self.generate_curl_command(stub, port);
                    self.copy_to_clipboard(&curl_cmd);
                    self.set_status(
                        "Curl command copied to clipboard".to_string(),
                        StatusLevel::Success,
                    );
                }
            }
        }
    }

    /// Show save file dialog
    pub fn show_save_dialog(&mut self, content: String, port: u16) {
        // Generate default filename
        let default_path = dirs::home_dir()
            .map(|h| h.join(format!("imposter-{}.json", port)))
            .unwrap_or_else(|| std::path::PathBuf::from(format!("imposter-{}.json", port)));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: format!("Save imposter :{} to file", port),
            action: FileAction::SaveExport { content, port },
        };
    }

    /// Expand tilde in path to home directory
    fn expand_path(path: &str) -> String {
        if let Some(rest) = path.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(rest).to_string_lossy().to_string();
            }
        } else if path == "~" {
            if let Some(home) = dirs::home_dir() {
                return home.to_string_lossy().to_string();
            }
        }
        path.to_string()
    }

    /// Save content to file
    pub fn save_to_file(&mut self, path: &str, content: &str) {
        let expanded_path = Self::expand_path(path);
        match std::fs::write(&expanded_path, content) {
            Ok(_) => {
                self.set_status(format!("Saved to {}", expanded_path), StatusLevel::Success);
                self.overlay = Overlay::None;
            }
            Err(e) => {
                self.set_status(format!("Failed to save: {}", e), StatusLevel::Error);
            }
        }
    }

    /// Confirm apply recorded stubs
    pub fn confirm_apply_recorded_stubs(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => *port,
            _ => return,
        };

        self.overlay = Overlay::Confirm {
            message: format!(
                "Apply recorded stubs to :{}?\nThis will remove proxy stubs and replace with recorded responses.",
                port
            ),
            action: PendingAction::ApplyRecordedStubs { port },
        };
    }

    /// Apply recorded stubs (remove proxy, keep recorded responses)
    pub async fn apply_recorded_stubs(&mut self, port: u16) {
        self.is_loading = true;

        // Get the imposter config with removeProxies=true
        match self.client.export_imposter(port, true).await {
            Ok(json) => {
                // Parse and re-apply
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(mut config) => {
                        // Turn off recording since we're no longer proxying
                        if let Some(obj) = config.as_object_mut() {
                            obj.insert(
                                "recordRequests".to_string(),
                                serde_json::Value::Bool(false),
                            );
                        }

                        // Delete and recreate imposter with new stubs
                        if let Err(e) = self.client.delete_imposter(port).await {
                            self.set_status(format!("Failed to apply: {}", e), StatusLevel::Error);
                            self.is_loading = false;
                            self.overlay = Overlay::None;
                            return;
                        }

                        // Recreate with the filtered config
                        let url = format!("{}/imposters", self.client.base_url());
                        let resp = self.client.client().post(url).json(&config).send().await;

                        match resp {
                            Ok(r) if r.status().is_success() => {
                                self.set_status(
                                    format!("Applied recorded stubs to :{}", port),
                                    StatusLevel::Success,
                                );
                                self.refresh().await;
                            }
                            Ok(r) => {
                                self.set_status(
                                    format!("Failed to apply: HTTP {}", r.status()),
                                    StatusLevel::Error,
                                );
                            }
                            Err(e) => {
                                self.set_status(
                                    format!("Failed to apply: {}", e),
                                    StatusLevel::Error,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        self.set_status(
                            format!("Failed to parse config: {}", e),
                            StatusLevel::Error,
                        );
                    }
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to export: {}", e), StatusLevel::Error);
            }
        }

        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Show import file dialog
    pub fn show_import_file_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        self.input_state.cursor_pos = default_path.len();
        self.input_state.file_path = default_path;
        self.overlay = Overlay::FilePathInput {
            prompt: "Import imposter from JSON file".to_string(),
            action: FileAction::ImportFile,
        };
    }

    /// Show import folder dialog
    pub fn show_import_folder_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        self.input_state.cursor_pos = default_path.len();
        self.input_state.file_path = default_path;
        self.overlay = Overlay::FilePathInput {
            prompt: "Import imposters from folder (*.json)".to_string(),
            action: FileAction::ImportFolder,
        };
    }

    /// Import imposter from file
    pub async fn import_from_file(&mut self, path: &str) {
        self.is_loading = true;
        let expanded_path = Self::expand_path(path);

        match std::fs::read_to_string(&expanded_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(config) => {
                    let url = format!("{}/imposters", self.client.base_url());
                    let resp = self.client.client().post(url).json(&config).send().await;

                    match resp {
                        Ok(r) if r.status().is_success() => {
                            self.set_status(
                                format!("Imported from {}", expanded_path),
                                StatusLevel::Success,
                            );
                            self.overlay = Overlay::None;
                            self.refresh().await;
                        }
                        Ok(r) => {
                            let body = r.text().await.unwrap_or_default();
                            self.set_status(
                                format!("Failed to import: {}", body),
                                StatusLevel::Error,
                            );
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to import: {}", e), StatusLevel::Error);
                        }
                    }
                }
                Err(e) => {
                    self.set_status(format!("Invalid JSON: {}", e), StatusLevel::Error);
                }
            },
            Err(e) => {
                self.set_status(format!("Failed to read file: {}", e), StatusLevel::Error);
            }
        }

        self.is_loading = false;
    }

    /// Import imposters from folder
    pub async fn import_from_folder(&mut self, folder: &str) {
        self.is_loading = true;
        let expanded_folder = Self::expand_path(folder);

        let path = std::path::Path::new(&expanded_folder);
        if !path.is_dir() {
            self.set_status(
                format!("{} is not a directory", expanded_folder),
                StatusLevel::Error,
            );
            self.is_loading = false;
            return;
        }

        let mut imported = 0;
        let mut failed = 0;

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                            let url = format!("{}/imposters", self.client.base_url());
                            let resp = self.client.client().post(url).json(&config).send().await;

                            if resp.map(|r| r.status().is_success()).unwrap_or(false) {
                                imported += 1;
                            } else {
                                failed += 1;
                            }
                        } else {
                            failed += 1;
                        }
                    }
                }
            }
        }

        if failed > 0 {
            self.set_status(
                format!("Imported {} imposters, {} failed", imported, failed),
                StatusLevel::Warning,
            );
        } else {
            self.set_status(
                format!("Imported {} imposters", imported),
                StatusLevel::Success,
            );
        }

        self.overlay = Overlay::None;
        self.refresh().await;
        self.is_loading = false;
    }

    /// Show export all dialog
    pub fn show_export_all_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.join("imposters.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("imposters.json"));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: "Export all imposters to file".to_string(),
            action: FileAction::ExportAll,
        };
    }

    /// Show export to folder dialog
    pub fn show_export_folder_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.join("imposters"))
            .unwrap_or_else(|| std::path::PathBuf::from("imposters"));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: "Export imposters to folder (one file per imposter)".to_string(),
            action: FileAction::ExportToFolder,
        };
    }

    /// Export all imposters to a single file
    pub async fn export_all_to_file(&mut self, path: &str) {
        self.is_loading = true;
        let expanded_path = Self::expand_path(path);

        match self.client.export_all_imposters().await {
            Ok(json) => match std::fs::write(&expanded_path, &json) {
                Ok(_) => {
                    self.set_status(
                        format!("Exported to {}", expanded_path),
                        StatusLevel::Success,
                    );
                    self.overlay = Overlay::None;
                }
                Err(e) => {
                    self.set_status(format!("Failed to write: {}", e), StatusLevel::Error);
                }
            },
            Err(e) => {
                self.set_status(format!("Failed to export: {}", e), StatusLevel::Error);
            }
        }

        self.is_loading = false;
    }

    /// Export imposters to individual files in a folder
    pub async fn export_to_folder(&mut self, folder: &str) {
        self.is_loading = true;
        let expanded_folder = Self::expand_path(folder);

        let path = std::path::Path::new(&expanded_folder);

        // Create folder if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(path) {
            self.set_status(
                format!("Failed to create folder: {}", e),
                StatusLevel::Error,
            );
            self.is_loading = false;
            return;
        }

        let mut exported = 0;
        let mut failed = 0;

        for imp in &self.imposters {
            match self.client.export_imposter(imp.port, false).await {
                Ok(json) => {
                    let filename = if let Some(name) = &imp.name {
                        format!("{}-{}.json", imp.port, name.replace(['/', '\\', ' '], "_"))
                    } else {
                        format!("{}.json", imp.port)
                    };
                    let file_path = path.join(filename);
                    if std::fs::write(&file_path, &json).is_ok() {
                        exported += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        if failed > 0 {
            self.set_status(
                format!("Exported {} imposters, {} failed", exported, failed),
                StatusLevel::Warning,
            );
        } else {
            self.set_status(
                format!("Exported {} imposters to {}", exported, folder),
                StatusLevel::Success,
            );
        }

        self.overlay = Overlay::None;
        self.is_loading = false;
    }

    /// Confirm clear requests
    pub fn confirm_clear_requests(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            self.overlay = Overlay::Confirm {
                message: format!("Clear all recorded requests for :{}?", port),
                action: PendingAction::ClearRequests { port },
            };
        }
    }

    /// Clear recorded requests
    pub async fn clear_requests(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.clear_requests(port).await {
            Ok(_) => {
                self.set_status(
                    "Cleared recorded requests".to_string(),
                    StatusLevel::Success,
                );
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to clear: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Start editing a stub
    pub fn start_stub_edit(&mut self) {
        let (port, idx) = match &self.view {
            View::ImposterDetail { port } => {
                if let Some(idx) = self.stub_list_state.selected() {
                    (*port, idx)
                } else {
                    return;
                }
            }
            View::StubDetail { port, index } => (*port, *index),
            _ => return,
        };

        if let Some(imp) = &self.current_imposter {
            if let Some(stub) = imp.stubs.get(idx) {
                let json = serde_json::to_string_pretty(stub).unwrap_or_default();
                self.stub_editor = Some(StubEditor::new(&json));
                self.navigate(View::StubEdit {
                    port,
                    index: Some(idx),
                });
            }
        }
    }

    /// Start creating a new stub
    pub fn start_stub_create(&mut self) {
        if let View::ImposterDetail { port } = self.view {
            let template = r#"{
  "predicates": [
    {"equals": {"method": "GET", "path": "/example"}}
  ],
  "responses": [
    {"is": {"statusCode": 200, "body": "Hello, World!"}}
  ]
}"#;
            self.stub_editor = Some(StubEditor::new(template));
            self.navigate(View::StubEdit { port, index: None });
        }
    }

    /// Save the current stub being edited
    pub async fn save_stub(&mut self) {
        if let Some(editor) = &mut self.stub_editor {
            if !editor.validate() {
                return;
            }

            if let Some(stub) = editor.get_stub() {
                if let View::StubEdit { port, index } = self.view {
                    self.is_loading = true;
                    let result = if let Some(idx) = index {
                        self.client.update_stub(port, idx, stub).await
                    } else {
                        self.client.add_stub(port, stub, None).await
                    };

                    match result {
                        Ok(_) => {
                            self.set_status("Stub saved".to_string(), StatusLevel::Success);
                            self.stub_editor = None;
                            self.go_back();
                            self.refresh().await;
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to save: {}", e), StatusLevel::Error);
                        }
                    }
                    self.is_loading = false;
                }
            }
        }
    }

    /// Cancel stub editing
    pub fn cancel_stub_edit(&mut self) {
        self.stub_editor = None;
        self.go_back();
    }

    /// Confirm delete stub
    pub fn confirm_delete_stub(&mut self) {
        if let View::ImposterDetail { port } = self.view {
            if let Some(idx) = self.stub_list_state.selected() {
                self.overlay = Overlay::Confirm {
                    message: format!("Delete stub #{} from :{}?", idx, port),
                    action: PendingAction::DeleteStub { port, index: idx },
                };
            }
        }
    }

    /// Delete a stub
    pub async fn delete_stub(&mut self, port: u16, index: usize) {
        self.is_loading = true;
        match self.client.delete_stub(port, index).await {
            Ok(_) => {
                self.set_status("Stub deleted".to_string(), StatusLevel::Success);
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to delete: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Switch focus between panes
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::Left => FocusArea::Right,
            FocusArea::Right => FocusArea::Left,
        };
    }

    /// Execute a pending action
    pub async fn execute_pending_action(&mut self) {
        if let Overlay::Confirm { action, .. } = &self.overlay.clone() {
            match action {
                PendingAction::DeleteImposter { port } => {
                    self.delete_imposter(*port).await;
                }
                PendingAction::DeleteStub { port, index } => {
                    self.delete_stub(*port, *index).await;
                }
                PendingAction::ClearRequests { port } => {
                    self.clear_requests(*port).await;
                }
                PendingAction::ClearProxyResponses { port } => {
                    self.clear_proxy_responses(*port).await;
                }
                PendingAction::ApplyRecordedStubs { port } => {
                    self.apply_recorded_stubs(*port).await;
                }
            }
        }
    }

    /// Get sparkline data for a specific imposter
    pub fn get_sparkline_data(&self, port: u16) -> Vec<u64> {
        self.metrics_history
            .iter()
            .filter_map(|s| s.per_imposter.get(&port).copied())
            .collect()
    }

    /// Calculate request rate between snapshots
    pub fn calculate_rates(&self) -> HashMap<u16, f64> {
        let mut rates = HashMap::new();

        if self.metrics_history.len() >= 2 {
            let recent: Vec<_> = self.metrics_history.iter().rev().take(2).collect();
            if let (Some(newer), Some(older)) = (recent.first(), recent.get(1)) {
                let time_diff = newer
                    .timestamp
                    .duration_since(older.timestamp)
                    .as_secs_f64();
                if time_diff > 0.0 {
                    for (port, count) in &newer.per_imposter {
                        if let Some(old_count) = older.per_imposter.get(port) {
                            let rate = (*count as f64 - *old_count as f64) / time_diff;
                            rates.insert(*port, rate.max(0.0));
                        }
                    }
                }
            }
        }

        rates
    }

    /// Handle keyboard input
    pub async fn handle_key_event(&mut self, key: KeyEvent) {
        // Handle overlays first
        match &self.overlay.clone() {
            Overlay::Help => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') => {
                        self.overlay = Overlay::None;
                        self.help_scroll = 0;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.help_scroll < self.help_max_scroll {
                            self.help_scroll += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        self.help_scroll = self.help_scroll.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        self.help_scroll = (self.help_scroll + 10).min(self.help_max_scroll);
                    }
                    KeyCode::Home => {
                        self.help_scroll = 0;
                    }
                    KeyCode::End => {
                        self.help_scroll = self.help_max_scroll;
                    }
                    _ => {}
                }
                return;
            }
            Overlay::Confirm { .. } => match key.code {
                KeyCode::Enter => {
                    self.execute_pending_action().await;
                    return;
                }
                KeyCode::Esc => {
                    self.overlay = Overlay::None;
                    return;
                }
                _ => return,
            },
            Overlay::Error { .. } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.overlay = Overlay::None;
                }
                return;
            }
            Overlay::Input { action, .. } => {
                self.handle_input_event(key, action.clone()).await;
                return;
            }
            Overlay::Export { content, port, .. } => {
                match key.code {
                    KeyCode::Esc => {
                        self.overlay = Overlay::None;
                        self.export_scroll_offset = 0;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.export_scroll_offset = self.export_scroll_offset.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let max_scroll = content.lines().count().saturating_sub(10) as u16;
                        self.export_scroll_offset = (self.export_scroll_offset + 1).min(max_scroll);
                    }
                    KeyCode::PageUp => {
                        self.export_scroll_offset = self.export_scroll_offset.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        let max_scroll = content.lines().count().saturating_sub(10) as u16;
                        self.export_scroll_offset =
                            (self.export_scroll_offset + 10).min(max_scroll);
                    }
                    KeyCode::Char('s') if port.is_some() => {
                        let content_clone = content.clone();
                        let port_val = port.unwrap();
                        self.export_scroll_offset = 0;
                        self.show_save_dialog(content_clone, port_val);
                    }
                    KeyCode::Char('c') if port.is_some() => {
                        let content_clone = content.clone();
                        self.copy_to_clipboard(&content_clone);
                    }
                    _ => {}
                }
                return;
            }
            Overlay::FilePathInput { action, .. } => {
                self.handle_file_path_input(key, action.clone()).await;
                return;
            }
            Overlay::Success { .. } => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::None => {}
        }

        // Handle editor mode
        if matches!(self.view, View::StubEdit { .. }) {
            self.handle_editor_event(key).await;
            return;
        }

        // Handle search mode
        if self.search_active {
            self.handle_search_input(key);
            return;
        }

        // Global keys
        match key.code {
            KeyCode::Char('?') => {
                self.overlay = Overlay::Help;
                self.help_scroll = 0;
                // Help text has ~75 lines, set max_scroll based on typical terminal height
                self.help_max_scroll = 50;
                return;
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query.clear();
                return;
            }
            KeyCode::Char('q') => {
                if matches!(self.view, View::ImposterList) {
                    self.should_quit = true;
                } else {
                    self.go_back();
                }
                return;
            }
            KeyCode::Esc => {
                self.go_back();
                return;
            }
            KeyCode::Char('r') => {
                self.refresh().await;
                return;
            }
            KeyCode::Char('T') => {
                self.cycle_theme();
                return;
            }
            _ => {}
        }

        // View-specific keys
        match self.view.clone() {
            View::ImposterList => self.handle_imposter_list_event(key).await,
            View::ImposterDetail { .. } => self.handle_imposter_detail_event(key).await,
            View::StubDetail { .. } => self.handle_stub_detail_event(key).await,
            View::Metrics => {}
            View::StubEdit { .. } => {}
        }
    }

    async fn handle_imposter_list_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Enter => self.enter_imposter_detail().await,
            KeyCode::Char('n') => self.show_create_imposter(),
            KeyCode::Char('p') => self.show_create_proxy_imposter(),
            KeyCode::Char('d') => self.confirm_delete_imposter(),
            KeyCode::Char('t') => self.toggle_imposter().await,
            KeyCode::Char('m') => self.navigate(View::Metrics),
            KeyCode::Char('i') => self.show_import_file_dialog(),
            KeyCode::Char('I') => self.show_import_folder_dialog(),
            KeyCode::Char('e') => self.show_export_all_dialog(),
            KeyCode::Char('E') => self.show_export_folder_dialog(),
            _ => {}
        }
    }

    async fn handle_imposter_detail_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Tab => self.toggle_focus(),
            KeyCode::Char('a') => self.start_stub_create(),
            KeyCode::Char('e') => self.start_stub_edit(),
            KeyCode::Char('d') => self.confirm_delete_stub(),
            KeyCode::Char('c') => self.confirm_clear_requests(),
            KeyCode::Char('C') => self.confirm_clear_proxy_responses(),
            KeyCode::Char('x') => self.export_imposter(true).await,
            KeyCode::Char('X') => self.export_imposter(false).await,
            KeyCode::Char('A') => self.confirm_apply_recorded_stubs(),
            KeyCode::Char('t') => self.toggle_imposter().await,
            KeyCode::Char('y') => self.copy_stub_as_curl(), // Yank curl command
            KeyCode::Enter => {
                if let View::ImposterDetail { port } = self.view {
                    if let Some(idx) = self.stub_list_state.selected() {
                        self.navigate(View::StubDetail { port, index: idx });
                    }
                }
            }
            _ => {}
        }
    }

    async fn handle_stub_detail_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('e') => self.start_stub_edit(),
            KeyCode::Char('d') => self.confirm_delete_stub(),
            KeyCode::Char('y') => self.copy_stub_as_curl(), // Yank curl command
            _ => {}
        }
    }

    async fn handle_editor_event(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => {
                    self.save_stub().await;
                    return;
                }
                KeyCode::Char('f') => {
                    if let Some(editor) = &mut self.stub_editor {
                        editor.format();
                    }
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.cancel_stub_edit();
            }
            _ => {
                // Get the action first
                let action = if let Some(editor) = &mut self.stub_editor {
                    editor.handle_key(key)
                } else {
                    None
                };

                // Handle clipboard actions (need separate borrows)
                match action {
                    Some(EditorAction::Copy(text)) | Some(EditorAction::Cut(text)) => {
                        self.copy_to_clipboard(&text);
                    }
                    Some(EditorAction::PasteRequest) => {
                        if let Some(text) = self.paste_from_clipboard() {
                            if let Some(editor) = &mut self.stub_editor {
                                editor.editor.paste(&text);
                            }
                        }
                    }
                    None => {}
                }

                // Validate after any changes
                if let Some(editor) = &mut self.stub_editor {
                    editor.validate();
                }
            }
        }
    }

    async fn handle_input_event(&mut self, key: KeyEvent, action: InputAction) {
        match action {
            InputAction::CreateImposter => self.handle_create_imposter_input(key).await,
            InputAction::CreateProxyImposter => self.handle_create_proxy_input(key).await,
        }
    }

    async fn handle_create_imposter_input(&mut self, key: KeyEvent) {
        // Handle Ctrl+V paste
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Some(text) = self.paste_from_clipboard() {
                match self.input_state.focus_field {
                    0 => {
                        // Port: only paste digits
                        let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                        self.input_state.port.push_str(&digits);
                    }
                    1 => self.input_state.name.push_str(&text),
                    2 => self.input_state.protocol.push_str(&text),
                    _ => {}
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => self.create_imposter().await,
            KeyCode::Tab => self.input_state.focus_field = (self.input_state.focus_field + 1) % 3,
            KeyCode::BackTab => {
                self.input_state.focus_field = if self.input_state.focus_field == 0 {
                    2
                } else {
                    self.input_state.focus_field - 1
                };
            }
            KeyCode::Backspace => match self.input_state.focus_field {
                0 => {
                    self.input_state.port.pop();
                }
                1 => {
                    self.input_state.name.pop();
                }
                2 => {
                    self.input_state.protocol.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.input_state.focus_field {
                    0 => {
                        if c.is_ascii_digit() {
                            self.input_state.port.push(c);
                        }
                    }
                    1 => {
                        self.input_state.name.push(c);
                    }
                    2 => {
                        self.input_state.protocol.push(c);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    async fn handle_create_proxy_input(&mut self, key: KeyEvent) {
        // Handle Ctrl+V paste
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Some(text) = self.paste_from_clipboard() {
                match self.input_state.focus_field {
                    0 => self.input_state.target_url.push_str(&text),
                    1 => {
                        // Port: only paste digits
                        let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                        self.input_state.port.push_str(&digits);
                    }
                    2 => self.input_state.name.push_str(&text),
                    _ => {}
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => self.create_proxy_imposter().await,
            KeyCode::Tab => self.input_state.focus_field = (self.input_state.focus_field + 1) % 4,
            KeyCode::BackTab => {
                self.input_state.focus_field = if self.input_state.focus_field == 0 {
                    3
                } else {
                    self.input_state.focus_field - 1
                };
            }
            KeyCode::Left if self.input_state.focus_field == 3 => {
                self.input_state.proxy_mode = if self.input_state.proxy_mode == 0 {
                    2
                } else {
                    self.input_state.proxy_mode - 1
                };
            }
            KeyCode::Right if self.input_state.focus_field == 3 => {
                self.input_state.proxy_mode = (self.input_state.proxy_mode + 1) % 3;
            }
            KeyCode::Backspace => match self.input_state.focus_field {
                0 => {
                    self.input_state.target_url.pop();
                }
                1 => {
                    self.input_state.port.pop();
                }
                2 => {
                    self.input_state.name.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.input_state.focus_field {
                    0 => {
                        self.input_state.target_url.push(c);
                    }
                    1 => {
                        if c.is_ascii_digit() {
                            self.input_state.port.push(c);
                        }
                    }
                    2 => {
                        self.input_state.name.push(c);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    async fn handle_file_path_input(&mut self, key: KeyEvent, action: FileAction) {
        // Handle Ctrl+V paste
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Some(text) = self.paste_from_clipboard() {
                // Insert pasted text at cursor position
                for c in text.chars() {
                    self.input_state
                        .file_path
                        .insert(self.input_state.cursor_pos, c);
                    self.input_state.cursor_pos += 1;
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => {
                let path = self.input_state.file_path.clone();
                if path.is_empty() {
                    return;
                }
                match action {
                    FileAction::SaveExport { content, .. } => self.save_to_file(&path, &content),
                    FileAction::ImportFile => self.import_from_file(&path).await,
                    FileAction::ImportFolder => self.import_from_folder(&path).await,
                    FileAction::ExportAll => self.export_all_to_file(&path).await,
                    FileAction::ExportToFolder => self.export_to_folder(&path).await,
                }
            }
            KeyCode::Left => {
                if self.input_state.cursor_pos > 0 {
                    self.input_state.cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if self.input_state.cursor_pos < self.input_state.file_path.len() {
                    self.input_state.cursor_pos += 1;
                }
            }
            KeyCode::Home => self.input_state.cursor_pos = 0,
            KeyCode::End => self.input_state.cursor_pos = self.input_state.file_path.len(),
            KeyCode::Backspace => {
                if self.input_state.cursor_pos > 0 {
                    self.input_state.cursor_pos -= 1;
                    self.input_state
                        .file_path
                        .remove(self.input_state.cursor_pos);
                }
            }
            KeyCode::Delete => {
                if self.input_state.cursor_pos < self.input_state.file_path.len() {
                    self.input_state
                        .file_path
                        .remove(self.input_state.cursor_pos);
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_state
                    .file_path
                    .insert(self.input_state.cursor_pos, c);
                self.input_state.cursor_pos += 1;
            }
            _ => {}
        }
    }

    /// Handle search input
    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_active = false;
                self.search_query.clear();
            }
            KeyCode::Enter => {
                self.search_active = false;
                // Keep the query for filtering, select first match
                if !self.search_query.is_empty() {
                    self.select_first_match();
                }
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.push(c);
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(text) = self.paste_from_clipboard() {
                    self.search_query.push_str(&text);
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.clear();
            }
            _ => {}
        }
    }

    /// Select the first matching item after search
    fn select_first_match(&mut self) {
        match &self.view {
            View::ImposterList => {
                let filtered = self.filtered_imposters();
                if !filtered.is_empty() {
                    // Find the index of the first matching imposter in the original list
                    if let Some(first) = filtered.first() {
                        if let Some(idx) = self.imposters.iter().position(|i| i.port == first.port)
                        {
                            self.imposter_list_state.select(Some(idx));
                        }
                    }
                }
            }
            View::ImposterDetail { .. } => {
                let filtered = self.filtered_stubs();
                if !filtered.is_empty() {
                    self.stub_list_state.select(Some(filtered[0]));
                }
            }
            _ => {}
        }
    }

    /// Get filtered imposters based on search query
    pub fn filtered_imposters(&self) -> Vec<&ImposterSummary> {
        if self.search_query.is_empty() {
            self.imposters.iter().collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.imposters
                .iter()
                .filter(|imp| {
                    imp.port.to_string().contains(&query)
                        || imp
                            .name
                            .as_ref()
                            .map(|n| n.to_lowercase().contains(&query))
                            .unwrap_or(false)
                        || imp.protocol.to_lowercase().contains(&query)
                })
                .collect()
        }
    }

    /// Get filtered stub indices based on search query
    pub fn filtered_stubs(&self) -> Vec<usize> {
        if self.search_query.is_empty() {
            if let Some(imp) = &self.current_imposter {
                (0..imp.stubs.len()).collect()
            } else {
                vec![]
            }
        } else {
            let query = self.search_query.to_lowercase();
            if let Some(imp) = &self.current_imposter {
                imp.stubs
                    .iter()
                    .enumerate()
                    .filter(|(_, stub)| {
                        // Match scenario name
                        stub.scenario_name.as_ref().map(|n| n.to_lowercase().contains(&query)).unwrap_or(false)
                            // Match predicates (path, method)
                            || stub.predicates.iter().any(|p| {
                                p.to_string().to_lowercase().contains(&query)
                            })
                            // Match response type
                            || stub.responses.iter().any(|r| {
                                r.to_string().to_lowercase().contains(&query)
                            })
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                vec![]
            }
        }
    }

    /// Check if an imposter matches the current search
    pub fn imposter_matches_search(&self, imp: &ImposterSummary) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        let query = self.search_query.to_lowercase();
        imp.port.to_string().contains(&query)
            || imp
                .name
                .as_ref()
                .map(|n| n.to_lowercase().contains(&query))
                .unwrap_or(false)
            || imp.protocol.to_lowercase().contains(&query)
    }

    /// Check if a stub index matches the current search
    pub fn stub_matches_search(&self, index: usize) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        self.filtered_stubs().contains(&index)
    }

    /// Clear search
    pub fn clear_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }
}

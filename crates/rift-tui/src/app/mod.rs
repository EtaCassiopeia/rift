//! Application state and logic for the TUI

use crate::api::{
    ApiClient, CreateImposterRequest, ImposterDetail, ImposterSummary, MetricsData, Stub,
};
use crate::theme::Theme;
use crate::validation::{validate_imposter_json, validate_stub_json, ValidationReport};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

mod commands;
mod events;
mod search;

/// Maximum number of metrics snapshots to keep for sparklines
const MAX_METRICS_HISTORY: usize = 60;

/// Current view/screen
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    ImposterList,
    ImposterDetail { port: u16 },
    StubDetail { port: u16, index: usize },
    StubEdit { port: u16, index: Option<usize> },
    RequestDetail { port: u16, index: usize },
    Config,
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
    ValidationResult {
        report: ValidationReport,
        action: ValidationAction,
    },
}

/// Actions to take after viewing validation results
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationAction {
    /// Import a file despite warnings
    ProceedWithImport { path: String, content: String },
    /// Editor validation - just informational
    EditorInfo,
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
pub(super) struct CurlRequestParts {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub query_params: Vec<(String, String)>,
    pub json_body_parts: Vec<(String, serde_json::Value)>,
    pub raw_body: Option<String>,
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

/// Actions that the editor may request (clipboard operations)
#[derive(Debug, Clone)]
pub enum EditorAction {
    Copy(String),
    Cut(String),
    PasteRequest,
}

/// Stub JSON editor backed by ratatui-textarea
pub struct StubEditor {
    pub editor: ratatui_textarea::TextArea<'static>,
    pub validation_error: Option<String>,
    pub validation_report: Option<ValidationReport>,
    pub original_json: String,
}

impl StubEditor {
    pub fn new(json: &str) -> Self {
        let lines: Vec<String> = json.lines().map(String::from).collect();
        let mut editor = ratatui_textarea::TextArea::new(lines);
        editor.set_line_number_style(
            ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
        );
        editor.set_cursor_line_style(ratatui::style::Style::default());
        editor.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" Edit Stub (Ctrl+S save, Ctrl+F format, Ctrl+L lint, Esc cancel) "),
        );
        let original_json = json.to_string();
        let mut stub_editor = Self {
            editor,
            validation_error: None,
            validation_report: None,
            original_json,
        };
        stub_editor.validate();
        stub_editor
    }

    /// Validate the JSON content using rift-lint
    pub fn validate(&mut self) -> bool {
        let content = self.editor.lines().join("\n");
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(val) => {
                self.validation_error = None;
                let json_str = serde_json::to_string_pretty(&val).unwrap_or(content);
                let report = validate_stub_json(&json_str);
                if report.has_issues() {
                    self.validation_error = Some(report.summary());
                }
                self.validation_report = Some(report);
                true
            }
            Err(e) => {
                self.validation_error = Some(format!("JSON error: {}", e));
                self.validation_report = None;
                false
            }
        }
    }

    /// Get the stub if valid
    pub fn get_stub(&self) -> Option<crate::api::Stub> {
        let content = self.editor.lines().join("\n");
        serde_json::from_str(&content).ok()
    }

    /// Format the JSON content
    pub fn format(&mut self) {
        let content = self.editor.lines().join("\n");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                let lines: Vec<String> = pretty.lines().map(String::from).collect();
                self.editor = ratatui_textarea::TextArea::new(lines);
                self.editor.set_line_number_style(
                    ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
                );
                self.editor
                    .set_cursor_line_style(ratatui::style::Style::default());
                self.editor.set_block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title(" Edit Stub (Ctrl+S save, Ctrl+F format, Ctrl+L lint, Esc cancel) "),
                );
            }
        }
    }

    /// Handle a key event. Returns Some(EditorAction) for clipboard operations, None otherwise.
    /// Ctrl+S, Ctrl+F, Ctrl+L must be intercepted by the caller BEFORE calling this.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<EditorAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => {
                    let yanked = self.editor.yank_text();
                    if !yanked.is_empty() {
                        return Some(EditorAction::Copy(yanked));
                    }
                    return None;
                }
                KeyCode::Char('x') => {
                    let yanked = self.editor.yank_text();
                    if !yanked.is_empty() {
                        self.editor.input(crossterm_key_to_input(key));
                        return Some(EditorAction::Cut(yanked));
                    }
                    return None;
                }
                KeyCode::Char('v') => {
                    return Some(EditorAction::PasteRequest);
                }
                _ => {}
            }
        }
        self.editor.input(crossterm_key_to_input(key));
        None
    }
}

/// Convert a `crossterm::event::KeyEvent` to `ratatui_textarea::Input`.
///
/// ratatui-textarea uses its own re-exported crossterm types which differ from
/// the standalone `crossterm` crate used by the rest of the app.
pub(super) fn crossterm_key_to_input(key: KeyEvent) -> ratatui_textarea::Input {
    use ratatui_textarea::{Input, Key};
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let k = match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter => Key::Enter,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => {
            return Input {
                key: Key::Tab,
                ctrl,
                alt,
                shift: true,
            }
        }
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Esc => Key::Esc,
        KeyCode::F(n) => Key::F(n),
        _ => Key::Null,
    };
    Input {
        key: k,
        ctrl,
        alt,
        shift,
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
    pub validation_scroll_offset: u16,
    pub help_scroll: u16,
    pub help_max_scroll: u16,

    // Config view
    pub server_config: Option<serde_json::Value>,

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
            validation_scroll_offset: 0,
            help_scroll: 0,
            help_max_scroll: 0,

            server_config: None,

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

    /// Get selected imposter
    pub fn selected_imposter(&self) -> Option<&ImposterSummary> {
        self.imposter_list_state
            .selected()
            .and_then(|i| self.imposters.get(i))
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
}

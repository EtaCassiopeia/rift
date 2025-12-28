//! UI rendering for the TUI

mod dialogs;
mod help;
mod imposter_detail;
mod imposters;
mod metrics;
mod stubs;

use crate::app::{App, Overlay, StatusLevel, View};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Main draw function
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main content
            Constraint::Length(4), // Status bar (2 lines + borders)
        ])
        .split(frame.area());

    draw_header(frame, app, chunks[0]);

    match &app.view {
        View::ImposterList => imposters::draw_list(frame, app, chunks[1]),
        View::ImposterDetail { port } => imposter_detail::draw(frame, app, *port, chunks[1]),
        View::StubDetail { port, index } => {
            stubs::draw_detail(frame, app, *port, *index, chunks[1])
        }
        View::StubEdit { .. } => stubs::draw_editor(frame, app, chunks[1]),
        View::Metrics => metrics::draw(frame, app, chunks[1]),
    }

    draw_status_bar(frame, app, chunks[2]);

    // Draw overlays on top
    match &app.overlay {
        Overlay::Help => {
            // Note: We can't mutate app here, so we return the max_scroll for the caller to update
            help::draw_overlay(frame, app.help_scroll);
        }
        Overlay::Confirm { message, .. } => dialogs::draw_confirm(frame, message),
        Overlay::Error { message } => dialogs::draw_error(frame, message),
        Overlay::Input { prompt, action } => dialogs::draw_input(frame, app, prompt, action),
        Overlay::Export {
            title,
            content,
            port,
        } => dialogs::draw_export(
            frame,
            title,
            content,
            app.export_scroll_offset,
            port.is_some(),
        ),
        Overlay::FilePathInput { prompt, .. } => dialogs::draw_file_path_input(frame, app, prompt),
        Overlay::Success { message } => dialogs::draw_success(frame, message),
        Overlay::None => {}
    }
}

/// Draw the header bar
fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let connection_status = if app.is_connected {
        Span::styled("● Connected", Style::default().fg(app.theme.success))
    } else {
        Span::styled("○ Disconnected", Style::default().fg(app.theme.error))
    };

    let loading = if app.is_loading {
        Span::styled(" ⟳", Style::default().fg(app.theme.warning))
    } else {
        Span::raw("")
    };

    let imposter_count = Span::styled(
        format!(" Imposters: {}", app.imposters.len()),
        Style::default().fg(app.theme.muted),
    );

    let title = Line::from(vec![
        Span::styled(
            " Rift TUI ",
            Style::default()
                .fg(app.theme.header_fg)
                .bg(app.theme.header_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        connection_status,
        loading,
        Span::raw(" │ "),
        Span::styled(&app.admin_url, Style::default().fg(app.theme.muted)),
        imposter_count,
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let paragraph = Paragraph::new(title).block(block);
    frame.render_widget(paragraph, area);
}

/// Draw the status bar (or search bar when active)
fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    // Show search bar when active
    if app.search_active || !app.search_query.is_empty() {
        draw_search_bar(frame, app, area);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    if let Some((msg, level, _)) = &app.status_message {
        let color = match level {
            StatusLevel::Info => app.theme.fg,
            StatusLevel::Success => app.theme.success,
            StatusLevel::Warning => app.theme.warning,
            StatusLevel::Error => app.theme.error,
        };
        let paragraph = Paragraph::new(Span::styled(
            format!(" {}", msg),
            Style::default().fg(color),
        ))
        .block(block)
        .alignment(Alignment::Left);
        frame.render_widget(paragraph, area);
    } else {
        let (commands1, commands2) = get_commands(&app.view);
        let line1 = build_command_line(&commands1, app);
        let lines = if let Some(cmds2) = commands2 {
            let line2 = build_command_line(&cmds2, app);
            vec![line1, line2]
        } else {
            vec![line1]
        };
        let paragraph = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left);
        frame.render_widget(paragraph, area);
    }
}

/// Command definition (key, label)
type Command = (&'static str, &'static str);

/// Build a nvim-style command line with [key] notation and separators
fn build_command_line(commands: &[Command], app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (key, label)) in commands.iter().enumerate() {
        if i > 0 {
            // Subtle separator
            spans.push(Span::styled(" │ ", Style::default().fg(app.theme.border)));
        }
        // Key in brackets with accent color
        spans.push(Span::styled(
            format!("[{}]", key),
            Style::default()
                .fg(app.theme.key_fg)
                .add_modifier(Modifier::BOLD),
        ));
        // Label in muted color
        spans.push(Span::styled(
            format!(" {}", label),
            Style::default().fg(app.theme.cmd_fg),
        ));
    }
    Line::from(spans)
}

/// Get context-sensitive commands as (key, label) pairs
fn get_commands(view: &View) -> (Vec<Command>, Option<Vec<Command>>) {
    match view {
        View::ImposterList => (
            vec![
                ("n", "New"),
                ("p", "Proxy"),
                ("d", "Del"),
                ("t", "Toggle"),
                ("m", "Metrics"),
                ("/", "Search"),
                ("T", "Theme"),
                ("?", "Help"),
                ("q", "Quit"),
            ],
            Some(vec![
                ("i", "Import"),
                ("I", "ImportDir"),
                ("e", "Export"),
                ("E", "ExportDir"),
            ]),
        ),
        View::ImposterDetail { .. } => (
            vec![
                ("a", "Add"),
                ("e", "Edit"),
                ("d", "Del"),
                ("y", "Curl"),
                ("t", "Toggle"),
                ("/", "Search"),
                ("?", "Help"),
            ],
            Some(vec![
                ("c", "ClearReq"),
                ("C", "ClearProxy"),
                ("x", "ExportStubs"),
                ("X", "ExportFull"),
                ("A", "Apply"),
            ]),
        ),
        View::StubDetail { .. } => (
            vec![
                ("e", "Edit"),
                ("d", "Delete"),
                ("y", "Curl"),
                ("Esc", "Back"),
                ("?", "Help"),
            ],
            None,
        ),
        View::StubEdit { .. } => (
            vec![
                ("^S", "Save"),
                ("^F", "Format"),
                ("^A", "SelAll"),
                ("^C", "Copy"),
                ("^X", "Cut"),
                ("^V", "Paste"),
                ("Esc", "Cancel"),
            ],
            None,
        ),
        View::Metrics => (vec![("r", "Refresh"), ("Esc", "Back"), ("?", "Help")], None),
    }
}

/// Draw the search bar
fn draw_search_bar(frame: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.search_active {
        app.theme.highlight_bg
    } else {
        app.theme.border
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Search prompt and query
    let cursor = if app.search_active { "█" } else { "" };
    let match_count = match &app.view {
        View::ImposterList => {
            let filtered = app.filtered_imposters();
            format!(" ({}/{})", filtered.len(), app.imposters.len())
        }
        View::ImposterDetail { .. } => {
            let filtered = app.filtered_stubs();
            let total = app
                .current_imposter
                .as_ref()
                .map(|i| i.stubs.len())
                .unwrap_or(0);
            format!(" ({}/{})", filtered.len(), total)
        }
        _ => String::new(),
    };

    let line = Line::from(vec![
        Span::styled(
            " /",
            Style::default()
                .fg(app.theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&app.search_query, Style::default().fg(app.theme.fg)),
        Span::styled(cursor, Style::default().fg(app.theme.highlight_bg)),
        Span::styled(&match_count, Style::default().fg(app.theme.muted)),
        Span::styled(
            if app.search_active {
                "  [Enter] search  [Esc] cancel  [Ctrl+U] clear"
            } else {
                "  [/] edit  [Esc] clear"
            },
            Style::default().fg(app.theme.muted),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, inner);
}

/// Calculate a centered rect for modals
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Format a number with thousands separator
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format uptime duration
pub fn format_uptime(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

//! Config/system view — shows server configuration and TUI uptime

use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Draw the server configuration view
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    // Uptime header
    let uptime = super::format_uptime(app.start_time.elapsed());
    let header_text = Line::from(vec![
        Span::styled(" TUI uptime: ", Style::default().fg(app.theme.muted)),
        Span::styled(
            uptime,
            Style::default()
                .fg(app.theme.fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   Connected to: ", Style::default().fg(app.theme.muted)),
        Span::styled(
            &app.admin_url,
            Style::default()
                .fg(app.theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let header = Paragraph::new(header_text).block(
        Block::default()
            .title(" System ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    );
    frame.render_widget(header, chunks[0]);

    // Config body
    let config_block = Block::default()
        .title(" Server Configuration ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    if let Some(cfg) = &app.server_config {
        let lines = render_json_as_lines(cfg, app);
        let paragraph = Paragraph::new(lines).block(config_block);
        frame.render_widget(paragraph, chunks[1]);
    } else {
        let paragraph = Paragraph::new(Span::styled(
            " No configuration loaded. Press [r] to refresh.",
            Style::default().fg(app.theme.muted),
        ))
        .block(config_block);
        frame.render_widget(paragraph, chunks[1]);
    }
}

fn render_json_as_lines<'a>(value: &serde_json::Value, app: &'a App) -> Vec<Line<'a>> {
    match value.as_object() {
        Some(map) => map
            .iter()
            .map(|(k, v)| {
                let val_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Null => "null".to_string(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{k:<24}"),
                        Style::default()
                            .fg(app.theme.key_fg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" │ ", Style::default().fg(app.theme.border)),
                    Span::styled(val_str, Style::default().fg(app.theme.fg)),
                ])
            })
            .collect(),
        None => {
            let json = serde_json::to_string_pretty(value).unwrap_or_default();
            json.lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(app.theme.fg),
                    ))
                })
                .collect()
        }
    }
}

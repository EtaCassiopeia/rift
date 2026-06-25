//! Request detail view — shows full recorded request JSON

use crate::app::App;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Draw the full request detail for a single recorded request
pub fn draw(frame: &mut Frame, app: &App, port: u16, index: usize, area: Rect) {
    let title = format!(" Request #{} for :{} ", index + 1, port);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let request = app
        .current_imposter
        .as_ref()
        .and_then(|imp| imp.requests.get(index));

    if let Some(req) = request {
        let json = serde_json::to_string_pretty(req).unwrap_or_else(|_| "{}".to_string());
        let lines: Vec<Line> = json
            .lines()
            .map(|l| {
                Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(app.theme.fg),
                ))
            })
            .collect();
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    } else {
        let paragraph = Paragraph::new(Span::styled(
            "Request not found",
            Style::default().fg(app.theme.muted),
        ))
        .block(block);
        frame.render_widget(paragraph, area);
    }
}

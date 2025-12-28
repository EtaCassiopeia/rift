//! Stub detail and editor views

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Draw stub detail view
pub fn draw_detail(frame: &mut Frame, app: &App, port: u16, index: usize, area: Rect) {
    let stub = app
        .current_imposter
        .as_ref()
        .and_then(|i| i.stubs.get(index));

    let title = format!(" Stub #{} (Port :{}) ", index, port);

    let content = if let Some(stub) = stub {
        serde_json::to_string_pretty(stub).unwrap_or_else(|_| "Error formatting stub".to_string())
    } else {
        "Stub not found".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let paragraph = Paragraph::new(syntax_highlight(&content, app))
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Draw the stub editor
pub fn draw_editor(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Editor
            Constraint::Length(3), // Validation/Help
        ])
        .split(area);

    // Editor
    if let Some(editor) = &app.stub_editor {
        let title = if matches!(app.view, crate::app::View::StubEdit { index: None, .. }) {
            " New Stub "
        } else {
            " Edit Stub "
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.highlight_bg));

        let inner = block.inner(chunks[0]);
        frame.render_widget(block, chunks[0]);

        // Render the text editor
        frame.render_widget(&editor.editor, inner);

        // Validation status and shortcuts
        let validation_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border));

        let validation_text = if let Some(err) = &editor.validation_error {
            vec![Line::from(vec![
                Span::styled(" ✗ ", Style::default().fg(app.theme.error)),
                Span::styled(err, Style::default().fg(app.theme.error)),
            ])]
        } else {
            vec![Line::from(vec![
                Span::styled(" ✓ ", Style::default().fg(app.theme.success)),
                Span::styled("Valid JSON", Style::default().fg(app.theme.success)),
                Span::styled(
                    "  │  ^S Save  ^F Format  ^A Select All  ^C Copy  ^X Cut  ^V Paste  Esc Cancel",
                    Style::default().fg(app.theme.muted),
                ),
            ])]
        };

        let validation = Paragraph::new(validation_text).block(validation_block);
        frame.render_widget(validation, chunks[1]);
    }
}

/// Apply basic JSON syntax highlighting
fn syntax_highlight<'a>(json: &str, app: &App) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    for line in json.lines() {
        let mut spans = Vec::new();
        let mut chars = line.chars().peekable();
        let mut current = String::new();
        let mut in_string = false;
        let mut is_key = false;

        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    if !current.is_empty() {
                        let style = if in_string {
                            if is_key {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Green)
                            }
                        } else {
                            Style::default().fg(app.theme.fg)
                        };
                        spans.push(Span::styled(current.clone(), style));
                        current.clear();
                    }

                    if !in_string {
                        in_string = true;
                        // Check if this is a key (followed eventually by :)
                        let rest: String = chars.clone().collect();
                        is_key = rest.contains(':');
                    } else {
                        in_string = false;
                    }

                    current.push(c);
                    let style = if is_key {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Green)
                    };
                    spans.push(Span::styled(current.clone(), style));
                    current.clear();
                }
                ':' | ',' | '{' | '}' | '[' | ']' => {
                    if !current.is_empty() {
                        let style = get_value_style(&current, app);
                        spans.push(Span::styled(current.clone(), style));
                        current.clear();
                    }
                    spans.push(Span::styled(
                        c.to_string(),
                        Style::default().fg(app.theme.muted),
                    ));
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty() {
            let style = get_value_style(&current, app);
            spans.push(Span::styled(current, style));
        }

        lines.push(Line::from(spans));
    }

    lines
}

/// Get style for JSON values
fn get_value_style(s: &str, app: &App) -> Style {
    let trimmed = s.trim();
    if trimmed == "true" || trimmed == "false" {
        Style::default().fg(Color::Yellow)
    } else if trimmed == "null" {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::ITALIC)
    } else if trimmed.parse::<f64>().is_ok() {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(app.theme.fg)
    }
}

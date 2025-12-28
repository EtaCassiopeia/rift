//! Metrics view with sparklines and bar charts

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph, Sparkline},
    Frame,
};

/// Draw the metrics view
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Summary panel
            Constraint::Min(10),   // Per-imposter metrics
        ])
        .split(area);

    draw_summary(frame, app, chunks[0]);
    draw_per_imposter(frame, app, chunks[1]);
}

/// Draw the summary metrics panel
fn draw_summary(frame: &mut Frame, app: &App, area: Rect) {
    let rates = app.calculate_rates();
    let total_rate: f64 = rates.values().sum();

    let uptime = super::format_uptime(app.start_time.elapsed());

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Imposters: ", Style::default().fg(app.theme.muted)),
            Span::styled(
                format!("{}", app.metrics.imposter_count),
                Style::default()
                    .fg(app.theme.fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("    │    ", Style::default().fg(app.theme.border)),
            Span::styled("Total Requests: ", Style::default().fg(app.theme.muted)),
            Span::styled(
                super::format_number(app.metrics.total_requests),
                Style::default()
                    .fg(app.theme.fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("    │    ", Style::default().fg(app.theme.border)),
            Span::styled("Rate: ", Style::default().fg(app.theme.muted)),
            Span::styled(
                format!("{:.1} req/s", total_rate),
                Style::default().fg(app.theme.success),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Session Uptime: ", Style::default().fg(app.theme.muted)),
            Span::styled(uptime, Style::default().fg(app.theme.fg)),
            Span::styled("    │    ", Style::default().fg(app.theme.border)),
            Span::styled("History: ", Style::default().fg(app.theme.muted)),
            Span::styled(
                format!("{} samples", app.metrics_history.len()),
                Style::default().fg(app.theme.fg),
            ),
        ]),
    ];

    let block = Block::default()
        .title(" Metrics Overview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Draw per-imposter metrics with bar chart and sparklines
fn draw_per_imposter(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_bar_chart(frame, app, chunks[0]);
    draw_sparklines(frame, app, chunks[1]);
}

/// Draw the bar chart comparing request counts
fn draw_bar_chart(frame: &mut Frame, app: &App, area: Rect) {
    let max_count = app
        .metrics
        .per_imposter
        .values()
        .map(|m| m.request_count)
        .max()
        .unwrap_or(1)
        .max(1);

    let bars: Vec<Bar> = app
        .imposters
        .iter()
        .filter_map(|imp| {
            app.metrics.per_imposter.get(&imp.port).map(|m| {
                Bar::default()
                    .value(m.request_count)
                    .label(Line::from(format!(":{}", imp.port)))
                    .text_value(super::format_number(m.request_count))
                    .style(Style::default().fg(app.theme.success))
            })
        })
        .collect();

    let bar_chart = BarChart::default()
        .block(
            Block::default()
                .title(" Request Counts ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .data(BarGroup::default().bars(&bars))
        .bar_width(8)
        .bar_gap(2)
        .max(max_count);

    frame.render_widget(bar_chart, area);
}

/// Draw sparklines for each imposter
fn draw_sparklines(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Request Trends ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.imposters.is_empty() {
        let msg = Paragraph::new("No imposters to display")
            .style(Style::default().fg(app.theme.muted))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    // Calculate rows for each imposter
    let num_imposters = app.imposters.len().min(10); // Max 10 sparklines
    let row_height = 3u16;
    let available_height = inner.height;
    let rows_that_fit = (available_height / row_height) as usize;
    let num_rows = num_imposters.min(rows_that_fit).max(1);

    let constraints: Vec<Constraint> = (0..num_rows)
        .map(|_| Constraint::Length(row_height))
        .collect();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, imp) in app.imposters.iter().take(num_rows).enumerate() {
        let row = rows[i];

        // Split row into label and sparkline
        let row_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(12), Constraint::Min(20)])
            .split(row);

        // Label
        let rates = app.calculate_rates();
        let rate = rates.get(&imp.port).copied().unwrap_or(0.0);
        let label = Paragraph::new(vec![
            Line::from(Span::styled(
                format!(":{}", imp.port),
                Style::default()
                    .fg(app.theme.fg)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("{:.1}/s", rate),
                Style::default().fg(if rate > 0.0 {
                    app.theme.success
                } else {
                    app.theme.muted
                }),
            )),
        ]);
        frame.render_widget(label, row_chunks[0]);

        // Sparkline
        let data = app.get_sparkline_data(imp.port);
        if !data.is_empty() {
            let sparkline = Sparkline::default()
                .data(&data)
                .style(Style::default().fg(app.theme.success));
            frame.render_widget(sparkline, row_chunks[1]);
        } else {
            let no_data = Paragraph::new("No data yet").style(Style::default().fg(app.theme.muted));
            frame.render_widget(no_data, row_chunks[1]);
        }
    }
}

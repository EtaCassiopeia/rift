//! Theme and color scheme for the TUI

use ratatui::style::Color;

/// Color theme for the TUI
#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
    pub border: Color,
    pub header_bg: Color,
    pub header_fg: Color,
    pub enabled: Color,
    pub disabled: Color,
    pub focus: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Reset,
            fg: Color::White,
            highlight_bg: Color::Blue,
            highlight_fg: Color::White,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            muted: Color::DarkGray,
            border: Color::Gray,
            header_bg: Color::Cyan,
            header_fg: Color::Black,
            enabled: Color::Green,
            disabled: Color::DarkGray,
            focus: Color::Yellow,
        }
    }
}

//! Theme and color scheme for the TUI

use ratatui::style::Color;

/// Available theme presets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
    #[default]
    Default,
    Dark,
    Light,
    Nord,
    Dracula,
}

impl ThemePreset {
    /// Get all available presets
    pub const ALL: &'static [ThemePreset] = &[
        ThemePreset::Default,
        ThemePreset::Dark,
        ThemePreset::Light,
        ThemePreset::Nord,
        ThemePreset::Dracula,
    ];

    /// Get the next theme in the cycle
    pub fn next(self) -> Self {
        match self {
            ThemePreset::Default => ThemePreset::Dark,
            ThemePreset::Dark => ThemePreset::Light,
            ThemePreset::Light => ThemePreset::Nord,
            ThemePreset::Nord => ThemePreset::Dracula,
            ThemePreset::Dracula => ThemePreset::Default,
        }
    }

    /// Get the theme name for display
    pub fn name(self) -> &'static str {
        match self {
            ThemePreset::Default => "Default",
            ThemePreset::Dark => "Dark",
            ThemePreset::Light => "Light",
            ThemePreset::Nord => "Nord",
            ThemePreset::Dracula => "Dracula",
        }
    }
}

/// Color theme for the TUI
#[derive(Debug, Clone)]
pub struct Theme {
    pub preset: ThemePreset,
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
    // Command bar colors (htop-style)
    pub key_bg: Color,
    pub key_fg: Color,
    pub cmd_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_preset(ThemePreset::Default)
    }
}

impl Theme {
    /// Create a theme from a preset
    pub fn from_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::Default => Self {
                preset,
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
                key_bg: Color::Cyan,
                key_fg: Color::Black,
                cmd_fg: Color::White,
            },
            ThemePreset::Dark => Self {
                preset,
                bg: Color::Rgb(30, 30, 30),
                fg: Color::Rgb(220, 220, 220),
                highlight_bg: Color::Rgb(60, 60, 80),
                highlight_fg: Color::White,
                success: Color::Rgb(80, 200, 80),
                warning: Color::Rgb(220, 180, 50),
                error: Color::Rgb(220, 80, 80),
                muted: Color::Rgb(100, 100, 100),
                border: Color::Rgb(70, 70, 70),
                header_bg: Color::Rgb(50, 50, 70),
                header_fg: Color::Rgb(200, 200, 220),
                enabled: Color::Rgb(80, 200, 80),
                disabled: Color::Rgb(80, 80, 80),
                focus: Color::Rgb(220, 180, 50),
                key_bg: Color::Rgb(70, 70, 90),
                key_fg: Color::Rgb(200, 200, 220),
                cmd_fg: Color::Rgb(150, 150, 150),
            },
            ThemePreset::Light => Self {
                preset,
                bg: Color::Reset,  // Use terminal default
                fg: Color::White,
                highlight_bg: Color::Rgb(100, 149, 237), // Cornflower blue
                highlight_fg: Color::White,
                success: Color::Rgb(50, 205, 50),  // Lime green
                warning: Color::Rgb(255, 200, 50), // Gold
                error: Color::Rgb(255, 99, 71),    // Tomato
                muted: Color::Rgb(169, 169, 169),  // Dark gray
                border: Color::Rgb(135, 135, 135),
                header_bg: Color::Rgb(65, 105, 225), // Royal blue
                header_fg: Color::White,
                enabled: Color::Rgb(50, 205, 50),
                disabled: Color::Rgb(128, 128, 128),
                focus: Color::Rgb(255, 215, 0),    // Gold
                key_bg: Color::Rgb(65, 105, 225),  // Royal blue
                key_fg: Color::White,
                cmd_fg: Color::Rgb(200, 200, 200),
            },
            ThemePreset::Nord => Self {
                preset,
                bg: Color::Rgb(46, 52, 64),      // nord0
                fg: Color::Rgb(236, 239, 244),   // nord6
                highlight_bg: Color::Rgb(76, 86, 106), // nord3
                highlight_fg: Color::Rgb(236, 239, 244),
                success: Color::Rgb(163, 190, 140), // nord14
                warning: Color::Rgb(235, 203, 139), // nord13
                error: Color::Rgb(191, 97, 106),   // nord11
                muted: Color::Rgb(76, 86, 106),    // nord3
                border: Color::Rgb(67, 76, 94),    // nord2
                header_bg: Color::Rgb(94, 129, 172), // nord10
                header_fg: Color::Rgb(236, 239, 244),
                enabled: Color::Rgb(163, 190, 140),
                disabled: Color::Rgb(76, 86, 106),
                focus: Color::Rgb(129, 161, 193),  // nord9
                key_bg: Color::Rgb(94, 129, 172),  // nord10
                key_fg: Color::Rgb(236, 239, 244), // nord6
                cmd_fg: Color::Rgb(216, 222, 233), // nord4
            },
            ThemePreset::Dracula => Self {
                preset,
                bg: Color::Rgb(40, 42, 54),       // background
                fg: Color::Rgb(248, 248, 242),    // foreground
                highlight_bg: Color::Rgb(68, 71, 90), // current line
                highlight_fg: Color::Rgb(248, 248, 242),
                success: Color::Rgb(80, 250, 123),  // green
                warning: Color::Rgb(241, 250, 140), // yellow
                error: Color::Rgb(255, 85, 85),     // red
                muted: Color::Rgb(98, 114, 164),    // comment
                border: Color::Rgb(68, 71, 90),
                header_bg: Color::Rgb(189, 147, 249), // purple
                header_fg: Color::Rgb(40, 42, 54),
                enabled: Color::Rgb(80, 250, 123),
                disabled: Color::Rgb(98, 114, 164),
                focus: Color::Rgb(255, 184, 108),   // orange
                key_bg: Color::Rgb(189, 147, 249),  // purple
                key_fg: Color::Rgb(40, 42, 54),     // background
                cmd_fg: Color::Rgb(248, 248, 242),  // foreground
            },
        }
    }

    /// Cycle to the next theme
    pub fn next(&mut self) {
        *self = Self::from_preset(self.preset.next());
    }
}

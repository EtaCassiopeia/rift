//! Simple multi-line text editor component with selection support
//!
//! A lightweight text editor widget that provides multi-line editing
//! with selection, copy, cut, and paste functionality.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Widget},
};

/// Position in the editor (row, column)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.row.cmp(&other.row) {
            std::cmp::Ordering::Equal => self.col.cmp(&other.col),
            ord => ord,
        }
    }
}

/// A simple multi-line text editor with selection support
#[derive(Debug, Clone)]
pub struct TextEditor {
    /// Lines of text
    lines: Vec<String>,
    /// Current cursor position
    cursor: Position,
    /// Selection anchor (start of selection, if any)
    selection_anchor: Option<Position>,
    /// Scroll offset for viewing
    scroll_offset: usize,
    /// Style for the editor
    style: Style,
    /// Cursor style
    cursor_style: Style,
    /// Selection style
    selection_style: Style,
    /// Line number style
    line_number_style: Style,
    /// Whether to show line numbers
    show_line_numbers: bool,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: Position::default(),
            selection_anchor: None,
            scroll_offset: 0,
            style: Style::default(),
            cursor_style: Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
            selection_style: Style::default().bg(Color::DarkGray).fg(Color::White),
            line_number_style: Style::default().fg(Color::DarkGray),
            show_line_numbers: true,
        }
    }
}

impl TextEditor {
    /// Create a new text editor with the given content
    pub fn new(content: &str) -> Self {
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };

        Self {
            lines,
            ..Default::default()
        }
    }

    /// Create from an iterator of lines
    pub fn from_lines<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let lines: Vec<String> = iter.into_iter().map(|s| s.as_ref().to_string()).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };

        Self {
            lines,
            ..Default::default()
        }
    }

    /// Get all lines
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Get the content as a single string
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Set the content
    pub fn set_content(&mut self, content: &str) {
        self.lines = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        self.cursor = Position::default();
        self.selection_anchor = None;
        self.scroll_offset = 0;
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Get the selected text
    pub fn get_selected_text(&self) -> Option<String> {
        // Early return if no selection
        self.selection_anchor?;
        let (start, end) = self.selection_bounds();

        if start == end {
            return None;
        }

        let mut result = String::new();

        for row in start.row..=end.row {
            if row >= self.lines.len() {
                break;
            }

            let line = &self.lines[row];
            let line_chars: Vec<char> = line.chars().collect();

            let col_start = if row == start.row { start.col } else { 0 };
            let col_end = if row == end.row {
                end.col
            } else {
                line_chars.len()
            };

            let selected: String = line_chars
                [col_start.min(line_chars.len())..col_end.min(line_chars.len())]
                .iter()
                .collect();
            result.push_str(&selected);

            if row < end.row {
                result.push('\n');
            }
        }

        Some(result)
    }

    /// Get selection bounds (start, end) in sorted order
    fn selection_bounds(&self) -> (Position, Position) {
        match self.selection_anchor {
            Some(anchor) => {
                if anchor <= self.cursor {
                    (anchor, self.cursor)
                } else {
                    (self.cursor, anchor)
                }
            }
            None => (self.cursor, self.cursor),
        }
    }

    /// Delete the selected text and return it
    fn delete_selection(&mut self) -> Option<String> {
        let text = self.get_selected_text()?;
        let (start, end) = self.selection_bounds();

        if start == end {
            self.selection_anchor = None;
            return None;
        }

        // Handle single-line selection
        if start.row == end.row {
            let line = &mut self.lines[start.row];
            let chars: Vec<char> = line.chars().collect();
            let new_line: String = chars[..start.col.min(chars.len())]
                .iter()
                .chain(chars[end.col.min(chars.len())..].iter())
                .collect();
            *line = new_line;
        } else {
            // Multi-line selection
            let start_line = &self.lines[start.row];
            let end_line = &self.lines[end.row];

            let start_chars: Vec<char> = start_line.chars().collect();
            let end_chars: Vec<char> = end_line.chars().collect();

            let new_line: String = start_chars[..start.col.min(start_chars.len())]
                .iter()
                .chain(end_chars[end.col.min(end_chars.len())..].iter())
                .collect();

            // Remove lines between start and end
            self.lines.drain(start.row..=end.row);
            self.lines.insert(start.row, new_line);
        }

        self.cursor = start;
        self.selection_anchor = None;
        self.adjust_scroll();

        Some(text)
    }

    /// Select all text
    pub fn select_all(&mut self) {
        self.selection_anchor = Some(Position::new(0, 0));
        let last_row = self.lines.len().saturating_sub(1);
        let last_col = self
            .lines
            .get(last_row)
            .map(|l| l.chars().count())
            .unwrap_or(0);
        self.cursor = Position::new(last_row, last_col);
    }

    /// Copy selected text to clipboard
    pub fn copy(&self) -> Option<String> {
        self.get_selected_text()
    }

    /// Cut selected text (delete and return)
    pub fn cut(&mut self) -> Option<String> {
        self.delete_selection()
    }

    /// Paste text at cursor position
    pub fn paste(&mut self, text: &str) {
        // Delete selection first if any
        self.delete_selection();

        // Insert the text
        let paste_lines: Vec<&str> = text.lines().collect();

        if paste_lines.is_empty() {
            return;
        }

        if paste_lines.len() == 1 {
            // Single line paste
            self.insert_str(paste_lines[0]);
        } else {
            // Multi-line paste
            let current_line = &self.lines[self.cursor.row];
            let chars: Vec<char> = current_line.chars().collect();

            let before: String = chars[..self.cursor.col.min(chars.len())].iter().collect();
            let after: String = chars[self.cursor.col.min(chars.len())..].iter().collect();

            // First line: before + first paste line
            self.lines[self.cursor.row] = before + paste_lines[0];

            // Middle lines
            for (i, line) in paste_lines[1..paste_lines.len() - 1].iter().enumerate() {
                self.lines.insert(self.cursor.row + 1 + i, line.to_string());
            }

            // Last line: last paste line + after
            let last_paste_line = paste_lines[paste_lines.len() - 1];
            self.lines.insert(
                self.cursor.row + paste_lines.len() - 1,
                last_paste_line.to_string() + &after,
            );

            // Update cursor position
            self.cursor.row += paste_lines.len() - 1;
            self.cursor.col = last_paste_line.chars().count();
        }

        self.adjust_scroll();
    }

    /// Insert a string at cursor position
    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char_internal(c);
        }
    }

    /// Handle a key event
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<EditorAction> {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match (key.code, ctrl, shift) {
            // Selection with Shift+Arrow
            (KeyCode::Left, false, true) => {
                self.start_or_extend_selection();
                self.move_cursor_left();
                return None;
            }
            (KeyCode::Right, false, true) => {
                self.start_or_extend_selection();
                self.move_cursor_right();
                return None;
            }
            (KeyCode::Up, false, true) => {
                self.start_or_extend_selection();
                self.move_cursor_up();
                return None;
            }
            (KeyCode::Down, false, true) => {
                self.start_or_extend_selection();
                self.move_cursor_down();
                return None;
            }
            (KeyCode::Home, false, true) => {
                self.start_or_extend_selection();
                self.cursor.col = 0;
                return None;
            }
            (KeyCode::End, false, true) => {
                self.start_or_extend_selection();
                self.cursor.col = self.current_line_len();
                return None;
            }

            // Ctrl+Shift+Home/End for document selection
            (KeyCode::Home, true, true) => {
                self.start_or_extend_selection();
                self.cursor = Position::new(0, 0);
                self.adjust_scroll();
                return None;
            }
            (KeyCode::End, true, true) => {
                self.start_or_extend_selection();
                let last_row = self.lines.len().saturating_sub(1);
                let last_col = self
                    .lines
                    .get(last_row)
                    .map(|l| l.chars().count())
                    .unwrap_or(0);
                self.cursor = Position::new(last_row, last_col);
                self.adjust_scroll();
                return None;
            }

            // Select all
            (KeyCode::Char('a'), true, false) => {
                self.select_all();
                return None;
            }

            // Copy
            (KeyCode::Char('c'), true, false) => {
                if let Some(text) = self.copy() {
                    return Some(EditorAction::Copy(text));
                }
                return None;
            }

            // Cut
            (KeyCode::Char('x'), true, false) => {
                if let Some(text) = self.cut() {
                    return Some(EditorAction::Cut(text));
                }
                return None;
            }

            // Paste - handled externally, just signal
            (KeyCode::Char('v'), true, false) => {
                return Some(EditorAction::PasteRequest);
            }

            // Navigation (clears selection)
            (KeyCode::Left, false, false) => {
                self.clear_selection();
                self.move_cursor_left();
            }
            (KeyCode::Right, false, false) => {
                self.clear_selection();
                self.move_cursor_right();
            }
            (KeyCode::Up, false, false) => {
                self.clear_selection();
                self.move_cursor_up();
            }
            (KeyCode::Down, false, false) => {
                self.clear_selection();
                self.move_cursor_down();
            }
            (KeyCode::Home, false, false) => {
                self.clear_selection();
                self.cursor.col = 0;
            }
            (KeyCode::End, false, false) => {
                self.clear_selection();
                self.cursor.col = self.current_line_len();
            }
            (KeyCode::PageUp, false, false) => {
                self.clear_selection();
                self.cursor.row = self.cursor.row.saturating_sub(10);
                self.cursor.col = self.cursor.col.min(self.current_line_len());
                self.adjust_scroll();
            }
            (KeyCode::PageDown, false, false) => {
                self.clear_selection();
                self.cursor.row = (self.cursor.row + 10).min(self.lines.len().saturating_sub(1));
                self.cursor.col = self.cursor.col.min(self.current_line_len());
                self.adjust_scroll();
            }

            // Word navigation
            (KeyCode::Left, true, false) => {
                self.clear_selection();
                self.move_word_left();
            }
            (KeyCode::Right, true, false) => {
                self.clear_selection();
                self.move_word_right();
            }

            // Editing
            (KeyCode::Char(c), false, _) => {
                self.delete_selection();
                self.insert_char_internal(c);
            }
            (KeyCode::Enter, false, false) => {
                self.delete_selection();
                self.insert_newline();
            }
            (KeyCode::Backspace, false, false) => {
                if self.has_selection() {
                    self.delete_selection();
                } else {
                    self.delete_char_before();
                }
            }
            (KeyCode::Delete, false, false) => {
                if self.has_selection() {
                    self.delete_selection();
                } else {
                    self.delete_char_after();
                }
            }
            (KeyCode::Tab, false, false) => {
                self.delete_selection();
                self.insert_char_internal(' ');
                self.insert_char_internal(' ');
            }

            // Line operations
            (KeyCode::Char('k'), true, false) => self.delete_line(),
            (KeyCode::Char('u'), true, false) => self.clear_line_before_cursor(),

            _ => {}
        }

        None
    }

    fn start_or_extend_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn current_line_len(&self) -> usize {
        self.lines
            .get(self.cursor.row)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    fn move_cursor_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.current_line_len();
            self.adjust_scroll();
        }
    }

    fn move_cursor_right(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor.col < line_len {
            self.cursor.col += 1;
        } else if self.cursor.row < self.lines.len() - 1 {
            self.cursor.row += 1;
            self.cursor.col = 0;
            self.adjust_scroll();
        }
    }

    fn move_cursor_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.cursor.col.min(self.current_line_len());
            self.adjust_scroll();
        }
    }

    fn move_cursor_down(&mut self) {
        if self.cursor.row < self.lines.len() - 1 {
            self.cursor.row += 1;
            self.cursor.col = self.cursor.col.min(self.current_line_len());
            self.adjust_scroll();
        }
    }

    fn move_word_left(&mut self) {
        if self.cursor.col == 0 {
            self.move_cursor_left();
            return;
        }

        let line = &self.lines[self.cursor.row];
        let chars: Vec<char> = line.chars().collect();
        let mut pos = self.cursor.col;

        // Skip whitespace
        while pos > 0
            && chars
                .get(pos - 1)
                .map(|c| c.is_whitespace())
                .unwrap_or(false)
        {
            pos -= 1;
        }
        // Skip word characters
        while pos > 0
            && chars
                .get(pos - 1)
                .map(|c| !c.is_whitespace())
                .unwrap_or(false)
        {
            pos -= 1;
        }

        self.cursor.col = pos;
    }

    fn move_word_right(&mut self) {
        let line = &self.lines[self.cursor.row];
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        let mut pos = self.cursor.col;

        // Skip word characters
        while pos < len && !chars[pos].is_whitespace() {
            pos += 1;
        }
        // Skip whitespace
        while pos < len && chars[pos].is_whitespace() {
            pos += 1;
        }

        if pos >= len && self.cursor.row < self.lines.len() - 1 {
            self.cursor.row += 1;
            self.cursor.col = 0;
            self.adjust_scroll();
        } else {
            self.cursor.col = pos;
        }
    }

    fn insert_char_internal(&mut self, c: char) {
        if let Some(line) = self.lines.get_mut(self.cursor.row) {
            let byte_pos = line
                .char_indices()
                .nth(self.cursor.col)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            line.insert(byte_pos, c);
            self.cursor.col += 1;
        }
    }

    fn insert_newline(&mut self) {
        if let Some(line) = self.lines.get_mut(self.cursor.row) {
            let byte_pos = line
                .char_indices()
                .nth(self.cursor.col)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            let rest = line[byte_pos..].to_string();
            line.truncate(byte_pos);
            self.lines.insert(self.cursor.row + 1, rest);
            self.cursor.row += 1;
            self.cursor.col = 0;
            self.adjust_scroll();
        }
    }

    fn delete_char_before(&mut self) {
        if self.cursor.col > 0 {
            if let Some(line) = self.lines.get_mut(self.cursor.row) {
                let byte_pos = line
                    .char_indices()
                    .nth(self.cursor.col - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let next_byte_pos = line
                    .char_indices()
                    .nth(self.cursor.col)
                    .map(|(i, _)| i)
                    .unwrap_or(line.len());
                line.replace_range(byte_pos..next_byte_pos, "");
                self.cursor.col -= 1;
            }
        } else if self.cursor.row > 0 {
            // Merge with previous line
            let current_line = self.lines.remove(self.cursor.row);
            self.cursor.row -= 1;
            self.cursor.col = self.lines[self.cursor.row].chars().count();
            self.lines[self.cursor.row].push_str(&current_line);
            self.adjust_scroll();
        }
    }

    fn delete_char_after(&mut self) {
        if let Some(line) = self.lines.get_mut(self.cursor.row) {
            if self.cursor.col < line.chars().count() {
                let byte_pos = line
                    .char_indices()
                    .nth(self.cursor.col)
                    .map(|(i, _)| i)
                    .unwrap_or(line.len());
                let next_byte_pos = line
                    .char_indices()
                    .nth(self.cursor.col + 1)
                    .map(|(i, _)| i)
                    .unwrap_or(line.len());
                line.replace_range(byte_pos..next_byte_pos, "");
            } else if self.cursor.row < self.lines.len() - 1 {
                // Merge with next line
                let next_line = self.lines.remove(self.cursor.row + 1);
                self.lines[self.cursor.row].push_str(&next_line);
            }
        }
    }

    fn delete_line(&mut self) {
        self.clear_selection();
        if self.lines.len() > 1 {
            self.lines.remove(self.cursor.row);
            if self.cursor.row >= self.lines.len() {
                self.cursor.row = self.lines.len() - 1;
            }
            self.cursor.col = self.cursor.col.min(self.current_line_len());
            self.adjust_scroll();
        } else {
            self.lines[0].clear();
            self.cursor.col = 0;
        }
    }

    fn clear_line_before_cursor(&mut self) {
        self.clear_selection();
        if let Some(line) = self.lines.get_mut(self.cursor.row) {
            let byte_pos = line
                .char_indices()
                .nth(self.cursor.col)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            line.replace_range(..byte_pos, "");
            self.cursor.col = 0;
        }
    }

    fn adjust_scroll(&mut self) {
        // This will be adjusted when rendering based on visible height
    }

    /// Check if a position is within the selection
    fn is_selected(&self, row: usize, col: usize) -> bool {
        if self.selection_anchor.is_none() {
            return false;
        }

        let pos = Position::new(row, col);
        let (start, end) = self.selection_bounds();
        pos >= start && pos < end
    }

    /// Render the editor into a buffer
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_with_block(area, buf, None);
    }

    /// Render the editor with an optional block
    pub fn render_with_block(&self, area: Rect, buf: &mut Buffer, block: Option<Block>) {
        let inner = if let Some(ref b) = block {
            let inner = b.inner(area);
            b.clone().render(area, buf);
            inner
        } else {
            area
        };

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let line_num_width = if self.show_line_numbers {
            format!("{}", self.lines.len()).len() + 2
        } else {
            0
        };

        let text_width = inner.width.saturating_sub(line_num_width as u16);
        let visible_height = inner.height as usize;

        // Calculate scroll offset to keep cursor visible
        let scroll = if self.cursor.row >= self.scroll_offset + visible_height {
            self.cursor.row - visible_height + 1
        } else if self.cursor.row < self.scroll_offset {
            self.cursor.row
        } else {
            self.scroll_offset
        };

        for (i, line) in self
            .lines
            .iter()
            .skip(scroll)
            .take(visible_height)
            .enumerate()
        {
            let y = inner.y + i as u16;
            let line_num = scroll + i;

            // Draw line number
            if self.show_line_numbers {
                let num_str = format!("{:>width$} ", line_num + 1, width = line_num_width - 2);
                let x = inner.x;
                for (j, c) in num_str.chars().enumerate() {
                    if x + (j as u16) < inner.x + inner.width {
                        buf[(x + j as u16, y)]
                            .set_char(c)
                            .set_style(self.line_number_style);
                    }
                }
            }

            // Draw line content
            let text_x = inner.x + line_num_width as u16;
            let chars: Vec<char> = line.chars().collect();

            for (j, c) in chars.iter().take(text_width as usize).enumerate() {
                let x = text_x + j as u16;
                if x < inner.x + inner.width {
                    let style = if line_num == self.cursor.row && j == self.cursor.col {
                        self.cursor_style
                    } else if self.is_selected(line_num, j) {
                        self.selection_style
                    } else {
                        self.style
                    };
                    buf[(x, y)].set_char(*c).set_style(style);
                }
            }

            // Draw cursor at end of line or on empty line
            if line_num == self.cursor.row && self.cursor.col >= chars.len() {
                let cursor_x = text_x + chars.len() as u16;
                if cursor_x < inner.x + inner.width {
                    buf[(cursor_x, y)]
                        .set_char(' ')
                        .set_style(self.cursor_style);
                }
            }

            // Extend selection highlight to end of line if selection spans multiple lines
            if self.selection_anchor.is_some() {
                let (start, end) = self.selection_bounds();
                if line_num >= start.row && line_num < end.row {
                    // This line is fully selected (up to end of line)
                    for j in chars.len()..text_width as usize {
                        let x = text_x + j as u16;
                        if x < inner.x + inner.width {
                            buf[(x, y)].set_style(self.selection_style);
                        }
                    }
                }
            }
        }
    }

    /// Set the style
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the cursor style
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Set the selection style
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = style;
        self
    }

    /// Set whether to show line numbers
    pub fn show_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }
}

/// Actions that the editor may request from the parent
#[derive(Debug, Clone)]
pub enum EditorAction {
    /// Request to copy text to system clipboard
    Copy(String),
    /// Request to cut text to system clipboard
    Cut(String),
    /// Request paste from system clipboard
    PasteRequest,
}

impl Widget for &TextEditor {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render(area, buf);
    }
}

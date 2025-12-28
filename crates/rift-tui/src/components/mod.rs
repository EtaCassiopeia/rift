//! Reusable UI components
//!
//! This module contains reusable components for the TUI:
//! - `TextEditor` - A multi-line text editor widget
//! - `popup` - Modal popup dialogs using tui-popup
//! - `input` - Text input using tui-prompts

mod text_editor;

pub use text_editor::{EditorAction, TextEditor};

// Re-export ecosystem widgets for convenience
pub use tui_popup::Popup;
pub use tui_prompts::{FocusState, State as PromptState, TextPrompt, TextState};

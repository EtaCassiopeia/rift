//! Event handling for the TUI

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

/// Events that can occur in the TUI
#[derive(Debug, Clone)]
pub enum Event {
    /// Keyboard input
    Key(KeyEvent),
    /// Tick for auto-refresh
    Tick,
    /// Terminal resize
    Resize(u16, u16),
}

/// Handles terminal events and produces Event stream
pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    _tx: mpsc::UnboundedSender<Event>,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();

        // Spawn event polling task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tx_clone.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {
                        // Poll for crossterm events
                        if event::poll(Duration::from_millis(0)).unwrap_or(false) {
                            if let Ok(evt) = event::read() {
                                let event = match evt {
                                    CrosstermEvent::Key(key) => Some(Event::Key(key)),
                                    CrosstermEvent::Resize(w, h) => Some(Event::Resize(w, h)),
                                    _ => None,
                                };
                                if let Some(e) = event {
                                    if tx_clone.send(e).is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Self { rx, _tx: tx }
    }

    /// Receive the next event
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

/// Helper to check if a key matches (case insensitive)
#[allow(dead_code)]
pub fn key_match(key: &KeyEvent, expected: char) -> bool {
    matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&expected))
}

/// Check for control key combination
#[allow(dead_code)]
pub fn ctrl_key(key: &KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&c))
}

/// Check for shift key combination
#[allow(dead_code)]
pub fn shift_key(key: &KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&c))
}

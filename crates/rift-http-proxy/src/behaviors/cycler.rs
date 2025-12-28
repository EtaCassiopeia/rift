//! Response cycling state management.

use parking_lot::RwLock;
use std::collections::HashMap;

/// State for a single rule's cycling behavior
#[derive(Default, Clone)]
struct RuleState {
    /// Current response index
    index: usize,
    /// Repeat counter (how many times current response has been used)
    repeat_count: usize,
}

/// Combined state for all rules - protected by a single lock to prevent deadlocks
#[derive(Default)]
struct CyclerState {
    rules: HashMap<String, RuleState>,
}

/// Tracks response cycling state per rule
///
/// Uses a single lock to protect all state, avoiding the deadlock that could occur
/// with multiple locks acquired in inconsistent order.
pub struct ResponseCycler {
    state: RwLock<CyclerState>,
}

impl Default for ResponseCycler {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseCycler {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(CyclerState::default()),
        }
    }

    /// Get current response index for a rule, handling repeat behavior
    /// Returns the index to use for this request
    pub fn get_response_index(
        &self,
        rule_id: &str,
        response_count: usize,
        repeat: Option<u32>,
    ) -> usize {
        if response_count == 0 {
            return 0;
        }

        let repeat_count = repeat.unwrap_or(1).max(1) as usize;

        let mut state = self.state.write();
        let rule_state = state.rules.entry(rule_id.to_string()).or_default();

        let current_index = rule_state.index % response_count;

        // Increment repeat counter
        rule_state.repeat_count += 1;

        // Check if we need to advance to next response
        if rule_state.repeat_count >= repeat_count {
            // Reset repeat counter and advance to next response
            rule_state.repeat_count = 0;
            rule_state.index = (current_index + 1) % response_count;
        }

        current_index
    }

    /// Reset cycling state for a rule
    pub fn reset(&self, rule_id: &str) {
        let mut state = self.state.write();
        if let Some(rule_state) = state.rules.get_mut(rule_id) {
            rule_state.index = 0;
            rule_state.repeat_count = 0;
        }
    }

    /// Reset all cycling state
    pub fn reset_all(&self) {
        self.state.write().rules.clear();
    }

    /// Peek at current response index without modifying state
    /// Used to check response type before committing to cycling
    pub fn peek_response_index(&self, rule_id: &str, response_count: usize) -> usize {
        if response_count == 0 {
            return 0;
        }

        let state = self.state.read();
        state
            .rules
            .get(rule_id)
            .map(|r| r.index % response_count)
            .unwrap_or(0)
    }

    /// Advance the cycler for a proxy response (which has no repeat behavior)
    /// This should be called after successfully handling a proxy response
    pub fn advance_for_proxy(&self, rule_id: &str, response_count: usize) {
        if response_count == 0 {
            return;
        }

        let mut state = self.state.write();
        let rule_state = state.rules.entry(rule_id.to_string()).or_default();

        let current_index = rule_state.index % response_count;
        rule_state.index = (current_index + 1) % response_count;
        // Reset repeat counter when advancing via proxy
        rule_state.repeat_count = 0;
    }

    /// Get response index with per-response repeat values
    /// Each response can have its own repeat count via _behaviors.repeat
    pub fn get_response_index_with_per_response_repeat<T: HasRepeatBehavior>(
        &self,
        rule_id: &str,
        responses: &[T],
    ) -> usize {
        if responses.is_empty() {
            return 0;
        }

        let mut state = self.state.write();
        let rule_state = state.rules.entry(rule_id.to_string()).or_default();

        let current_index = rule_state.index % responses.len();

        // Get repeat value for current response
        let repeat_count = responses[current_index].get_repeat().unwrap_or(1).max(1) as usize;

        // Increment repeat counter
        rule_state.repeat_count += 1;

        // Check if we should advance to next response
        if rule_state.repeat_count >= repeat_count {
            // Reset repeat counter and advance to next response
            rule_state.repeat_count = 0;
            rule_state.index = (current_index + 1) % responses.len();
        }

        current_index
    }
}

/// Trait for types that can have a repeat behavior
pub trait HasRepeatBehavior {
    fn get_repeat(&self) -> Option<u32>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_cycler_basic() {
        let cycler = ResponseCycler::new();

        // With 3 responses, no repeat
        assert_eq!(cycler.get_response_index("rule1", 3, None), 0);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 1);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 2);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 0); // Wrap around
    }

    #[test]
    fn test_response_cycler_with_repeat() {
        let cycler = ResponseCycler::new();

        // With 2 responses, repeat=3
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1); // Advance after 3 repeats
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 1);
        assert_eq!(cycler.get_response_index("rule1", 2, Some(3)), 0); // Wrap around
    }

    #[test]
    fn test_response_cycler_independent_rules() {
        let cycler = ResponseCycler::new();

        // Different rules should have independent state
        assert_eq!(cycler.get_response_index("rule1", 3, None), 0);
        assert_eq!(cycler.get_response_index("rule2", 3, None), 0);
        assert_eq!(cycler.get_response_index("rule1", 3, None), 1);
        assert_eq!(cycler.get_response_index("rule2", 3, None), 1);
    }

    #[test]
    fn test_response_cycler_peek() {
        let cycler = ResponseCycler::new();

        // Peek should not modify state
        assert_eq!(cycler.peek_response_index("rule1", 3), 0);
        assert_eq!(cycler.peek_response_index("rule1", 3), 0);

        // After actual get, peek should reflect new state
        cycler.get_response_index("rule1", 3, None);
        assert_eq!(cycler.peek_response_index("rule1", 3), 1);
    }

    #[test]
    fn test_response_cycler_reset() {
        let cycler = ResponseCycler::new();

        cycler.get_response_index("rule1", 3, None);
        cycler.get_response_index("rule1", 3, None);
        assert_eq!(cycler.peek_response_index("rule1", 3), 2);

        cycler.reset("rule1");
        assert_eq!(cycler.peek_response_index("rule1", 3), 0);
    }

    #[test]
    fn test_response_cycler_advance_for_proxy() {
        let cycler = ResponseCycler::new();

        assert_eq!(cycler.peek_response_index("rule1", 3), 0);
        cycler.advance_for_proxy("rule1", 3);
        assert_eq!(cycler.peek_response_index("rule1", 3), 1);
        cycler.advance_for_proxy("rule1", 3);
        assert_eq!(cycler.peek_response_index("rule1", 3), 2);
        cycler.advance_for_proxy("rule1", 3);
        assert_eq!(cycler.peek_response_index("rule1", 3), 0); // Wrap around
    }

    #[test]
    fn test_response_cycler_zero_responses() {
        let cycler = ResponseCycler::new();

        // Should handle zero responses gracefully
        assert_eq!(cycler.get_response_index("rule1", 0, None), 0);
        assert_eq!(cycler.peek_response_index("rule1", 0), 0);
    }

    struct MockResponse {
        repeat: Option<u32>,
    }

    impl HasRepeatBehavior for MockResponse {
        fn get_repeat(&self) -> Option<u32> {
            self.repeat
        }
    }

    #[test]
    fn test_per_response_repeat() {
        let cycler = ResponseCycler::new();

        // First response repeats 2x, second repeats 3x
        let responses = vec![
            MockResponse { repeat: Some(2) },
            MockResponse { repeat: Some(3) },
        ];

        // First response, repeat 2x
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            0
        );
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            0
        );

        // Second response, repeat 3x
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            1
        );
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            1
        );
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            1
        );

        // Back to first response
        assert_eq!(
            cycler.get_response_index_with_per_response_repeat("rule1", &responses),
            0
        );
    }
}

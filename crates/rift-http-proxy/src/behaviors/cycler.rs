//! Response cycling state management.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tracks response cycling state per rule
pub struct ResponseCycler {
    /// Current response index per rule
    indices: RwLock<HashMap<String, AtomicUsize>>,
    /// Repeat counters per rule (how many times current response has been used)
    repeat_counters: RwLock<HashMap<String, AtomicUsize>>,
}

impl Default for ResponseCycler {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseCycler {
    pub fn new() -> Self {
        Self {
            indices: RwLock::new(HashMap::new()),
            repeat_counters: RwLock::new(HashMap::new()),
        }
    }

    /// Get current response index for a rule, handling repeat behavior
    /// Returns the index and whether it advanced to a new response
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

        // Get or create the index and counter for this rule
        let indices = self.indices.read();
        let counters = self.repeat_counters.read();

        let current_index = indices
            .get(rule_id)
            .map(|i| i.load(Ordering::SeqCst))
            .unwrap_or(0);

        let _current_repeat = counters
            .get(rule_id)
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(0);

        // Drop read locks
        drop(indices);
        drop(counters);

        // Increment repeat counter
        let mut counters = self.repeat_counters.write();
        let counter = counters
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let new_repeat = counter.fetch_add(1, Ordering::SeqCst) + 1;

        // Check if we need to advance to next response
        if new_repeat >= repeat_count {
            // Reset repeat counter
            counter.store(0, Ordering::SeqCst);

            // Advance to next response
            let mut indices = self.indices.write();
            let index = indices
                .entry(rule_id.to_string())
                .or_insert_with(|| AtomicUsize::new(0));

            let next_index = (current_index + 1) % response_count;
            index.store(next_index, Ordering::SeqCst);

            current_index % response_count
        } else {
            current_index % response_count
        }
    }

    /// Reset cycling state for a rule
    #[allow(dead_code)]
    pub fn reset(&self, rule_id: &str) {
        if let Some(index) = self.indices.write().get(rule_id) {
            index.store(0, Ordering::SeqCst);
        }
        if let Some(counter) = self.repeat_counters.write().get(rule_id) {
            counter.store(0, Ordering::SeqCst);
        }
    }

    /// Reset all cycling state
    #[allow(dead_code)]
    pub fn reset_all(&self) {
        self.indices.write().clear();
        self.repeat_counters.write().clear();
    }

    /// Peek at current response index without modifying state
    /// Used to check response type before committing to cycling
    pub fn peek_response_index(&self, rule_id: &str, response_count: usize) -> usize {
        if response_count == 0 {
            return 0;
        }

        let indices = self.indices.read();
        if let Some(index_entry) = indices.get(rule_id) {
            index_entry.load(Ordering::SeqCst) % response_count
        } else {
            0
        }
    }

    /// Advance the cycler for a proxy response (which has no repeat behavior)
    /// This should be called after successfully handling a proxy response
    pub fn advance_for_proxy(&self, rule_id: &str, response_count: usize) {
        if response_count == 0 {
            return;
        }

        let mut indices = self.indices.write();
        let index_entry = indices
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let current_index = index_entry.load(Ordering::SeqCst) % response_count;
        let next_index = (current_index + 1) % response_count;
        index_entry.store(next_index, Ordering::SeqCst);
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

        // Get current state
        let mut indices = self.indices.write();
        let mut counters = self.repeat_counters.write();

        let index_entry = indices
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));
        let counter_entry = counters
            .entry(rule_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0));

        let current_index = index_entry.load(Ordering::SeqCst) % responses.len();
        let current_repeat = counter_entry.load(Ordering::SeqCst);

        // Get repeat value for current response
        let repeat_count = responses[current_index].get_repeat().unwrap_or(1).max(1) as usize;

        // Increment repeat counter
        let new_repeat = current_repeat + 1;

        // Return current index and decide if we should advance
        if new_repeat >= repeat_count {
            // Reset repeat counter
            counter_entry.store(0, Ordering::SeqCst);
            // Advance to next response for next call
            let next_index = (current_index + 1) % responses.len();
            index_entry.store(next_index, Ordering::SeqCst);
        } else {
            // Increment repeat counter for next call
            counter_entry.store(new_repeat, Ordering::SeqCst);
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
}

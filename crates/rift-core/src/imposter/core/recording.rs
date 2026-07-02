//! Recorded-request storage: capture, retrieval, retention, and request counters.
//!
//! Part of the `Imposter` implementation; see `core/mod.rs` for the struct definition.

use super::*;

impl Imposter {
    /// Record a request. Evicts the oldest entry when the cap is reached.
    pub fn record_request(&self, req: &RecordedRequest) {
        if self.config.record_requests {
            let mut requests = self.recorded_requests.write();
            if requests.len() >= MAX_RECORDED_REQUESTS {
                tracing::warn!(
                    port = self.config.port,
                    max = MAX_RECORDED_REQUESTS,
                    "Recorded requests cap reached; oldest entry evicted"
                );
                requests.remove(0);
            }
            requests.push(req.clone());
        }
    }

    /// Get recorded requests
    pub fn get_recorded_requests(&self) -> Vec<RecordedRequest> {
        self.recorded_requests.read().clone()
    }

    /// Clear recorded requests
    pub fn clear_recorded_requests(&self) {
        self.recorded_requests.write().clear();
        // Reset request count to match Mountebank behavior
        self.request_count.store(0, Ordering::SeqCst);
    }

    /// Retain only the recorded requests for which `keep` returns true.
    /// Used for targeted clears (a single correlated slice); unlike
    /// `clear_recorded_requests` it does not reset the total request count,
    /// since other slices' requests remain.
    pub fn retain_recorded_requests<F: Fn(&RecordedRequest) -> bool>(&self, keep: F) {
        self.recorded_requests.write().retain(|r| keep(r));
    }

    /// Clear saved proxy responses
    pub fn clear_proxy_responses(&self) {
        self.recording_store.clear();
    }

    /// Increment request count
    pub fn increment_request_count(&self) -> u64 {
        self.request_count.fetch_add(1, Ordering::SeqCst)
    }

    /// Get request count
    pub fn get_request_count(&self) -> u64 {
        self.request_count.load(Ordering::SeqCst)
    }
}

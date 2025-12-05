//! Recording store for proxy responses.

use super::mode::ProxyMode;
use super::stub_generator::generate_stub;
use super::types::{RecordedResponse, RequestSignature};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

/// Recording store for proxy responses
pub struct RecordingStore {
    /// Recorded responses by request signature
    responses: RwLock<HashMap<RequestSignature, Vec<RecordedResponse>>>,
    /// Mode-specific behavior
    mode: ProxyMode,
}

impl RecordingStore {
    pub fn new(mode: ProxyMode) -> Self {
        Self {
            responses: RwLock::new(HashMap::new()),
            mode,
        }
    }

    /// Get the recording mode
    pub fn mode(&self) -> ProxyMode {
        self.mode
    }

    /// Record a response (for proxyOnce/proxyAlways modes)
    pub fn record(&self, signature: RequestSignature, response: RecordedResponse) {
        match self.mode {
            ProxyMode::ProxyOnce => {
                // Only record if not already recorded
                let mut store = self.responses.write();
                store.entry(signature).or_insert_with(|| vec![response]);
            }
            ProxyMode::ProxyAlways => {
                // Always record, append to list
                let mut store = self.responses.write();
                store.entry(signature).or_default().push(response);
            }
            ProxyMode::ProxyTransparent => {
                // Never record
            }
        }
    }

    /// Get recorded response for replay
    pub fn get_recorded(&self, signature: &RequestSignature) -> Option<RecordedResponse> {
        let store = self.responses.read();
        store
            .get(signature)
            .and_then(|responses| responses.first().cloned())
    }

    /// Check if should proxy or replay
    pub fn should_proxy(&self, signature: &RequestSignature) -> bool {
        match self.mode {
            ProxyMode::ProxyOnce => {
                // Proxy only if not recorded
                !self.responses.read().contains_key(signature)
            }
            ProxyMode::ProxyAlways => true,
            ProxyMode::ProxyTransparent => true,
        }
    }

    /// Get all recorded responses (for export)
    #[allow(dead_code)] // Public API for future use (mb replay export)
    pub fn get_all(&self) -> HashMap<RequestSignature, Vec<RecordedResponse>> {
        self.responses.read().clone()
    }

    /// Clear all recordings
    #[allow(dead_code)] // Public API for future use (admin endpoints)
    pub fn clear(&self) {
        self.responses.write().clear();
    }

    /// Get number of recorded signatures
    #[allow(dead_code)] // Public API for future use (metrics/debugging)
    pub fn len(&self) -> usize {
        self.responses.read().len()
    }

    /// Check if empty
    #[allow(dead_code)] // Public API for future use (metrics/debugging)
    pub fn is_empty(&self) -> bool {
        self.responses.read().is_empty()
    }

    /// Save recordings to file (JSON format)
    #[allow(dead_code)] // Public API for persistence
    pub fn save_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        let data = self.responses.read();
        let serializable: Vec<_> = data
            .iter()
            .map(|(sig, responses)| (sig.clone(), responses.clone()))
            .collect();

        let json = serde_json::to_string_pretty(&serializable)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        fs::write(path, json)?;
        info!("Saved {} recordings to {:?}", data.len(), path);
        Ok(())
    }

    /// Load recordings from file (JSON format)
    #[allow(dead_code)] // Public API for persistence
    pub fn load_from_file(&self, path: &Path) -> Result<usize, std::io::Error> {
        if !path.exists() {
            debug!("Recording file {:?} does not exist, starting fresh", path);
            return Ok(0);
        }

        let json = fs::read_to_string(path)?;
        let data: Vec<(RequestSignature, Vec<RecordedResponse>)> = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        let count = data.len();
        let mut store = self.responses.write();
        for (sig, responses) in data {
            store.insert(sig, responses);
        }

        info!("Loaded {} recordings from {:?}", count, path);
        Ok(count)
    }

    /// Export all recordings as Mountebank-compatible stubs
    #[allow(dead_code)] // Public API for mb replay export
    pub fn export_as_stubs(
        &self,
        include_method: bool,
        include_path: bool,
        include_query: bool,
        include_headers: &[String],
    ) -> Vec<serde_json::Value> {
        let store = self.responses.read();
        store
            .iter()
            .flat_map(|(sig, responses)| {
                responses.iter().map(move |resp| {
                    generate_stub(
                        sig,
                        resp,
                        include_method,
                        include_path,
                        include_query,
                        include_headers,
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unix_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    #[test]
    fn test_proxy_once_records_first_only() {
        let store = RecordingStore::new(ProxyMode::ProxyOnce);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        // First response should be recorded
        let resp1 = RecordedResponse {
            status: 200,
            headers: HashMap::new(),
            body: b"first".to_vec(),
            latency_ms: Some(100),
            timestamp_secs: unix_timestamp(),
        };
        store.record(sig.clone(), resp1);

        // Second response should NOT be recorded (proxyOnce)
        let resp2 = RecordedResponse {
            status: 201,
            headers: HashMap::new(),
            body: b"second".to_vec(),
            latency_ms: Some(50),
            timestamp_secs: unix_timestamp(),
        };
        store.record(sig.clone(), resp2);

        // Should return first response
        let recorded = store.get_recorded(&sig).unwrap();
        assert_eq!(recorded.status, 200);
        assert_eq!(recorded.body, b"first");
    }

    #[test]
    fn test_proxy_always_records_all() {
        let store = RecordingStore::new(ProxyMode::ProxyAlways);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        store.record(
            sig.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"first".to_vec(),
                latency_ms: Some(100),
                timestamp_secs: unix_timestamp(),
            },
        );

        store.record(
            sig.clone(),
            RecordedResponse {
                status: 201,
                headers: HashMap::new(),
                body: b"second".to_vec(),
                latency_ms: Some(50),
                timestamp_secs: unix_timestamp(),
            },
        );

        // Should have 2 recordings
        let all = store.get_all();
        assert_eq!(all.get(&sig).unwrap().len(), 2);
    }

    #[test]
    fn test_proxy_transparent_never_records() {
        let store = RecordingStore::new(ProxyMode::ProxyTransparent);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        store.record(
            sig.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"test".to_vec(),
                latency_ms: Some(100),
                timestamp_secs: unix_timestamp(),
            },
        );

        // Should NOT be recorded
        assert!(store.get_recorded(&sig).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn test_should_proxy() {
        let store = RecordingStore::new(ProxyMode::ProxyOnce);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        // Should proxy before recording
        assert!(store.should_proxy(&sig));

        store.record(
            sig.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"test".to_vec(),
                latency_ms: Some(100),
                timestamp_secs: unix_timestamp(),
            },
        );

        // Should NOT proxy after recording (replay instead)
        assert!(!store.should_proxy(&sig));
    }

    #[test]
    fn test_request_signature_with_query() {
        let sig1 = RequestSignature::new("GET", "/test", Some("a=1&b=2"), &[]);
        let sig2 = RequestSignature::new("GET", "/test", Some("a=1&b=2"), &[]);
        let sig3 = RequestSignature::new("GET", "/test", Some("a=1&b=3"), &[]);

        // Same signature should be equal
        assert_eq!(sig1, sig2);

        // Different query should be different
        assert_ne!(sig1, sig3);

        // Store should differentiate by query
        let store = RecordingStore::new(ProxyMode::ProxyOnce);
        store.record(
            sig1.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"response1".to_vec(),
                latency_ms: Some(10),
                timestamp_secs: unix_timestamp(),
            },
        );

        // sig2 should match sig1
        assert!(store.get_recorded(&sig2).is_some());

        // sig3 should not match
        assert!(store.get_recorded(&sig3).is_none());
    }

    #[test]
    fn test_request_signature_with_method() {
        let get_sig = RequestSignature::new("GET", "/test", None, &[]);
        let post_sig = RequestSignature::new("POST", "/test", None, &[]);

        // Different methods should produce different signatures
        assert_ne!(get_sig, post_sig);

        let store = RecordingStore::new(ProxyMode::ProxyOnce);
        store.record(
            get_sig.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"GET response".to_vec(),
                latency_ms: Some(10),
                timestamp_secs: unix_timestamp(),
            },
        );

        // GET should have recording
        assert!(store.get_recorded(&get_sig).is_some());

        // POST should not have recording
        assert!(store.get_recorded(&post_sig).is_none());
    }

    #[test]
    fn test_proxy_always_should_always_proxy() {
        let store = RecordingStore::new(ProxyMode::ProxyAlways);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        // Should always proxy even after recording
        assert!(store.should_proxy(&sig));

        store.record(
            sig.clone(),
            RecordedResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"test".to_vec(),
                latency_ms: Some(100),
                timestamp_secs: unix_timestamp(),
            },
        );

        // Still should proxy (proxyAlways always proxies)
        assert!(store.should_proxy(&sig));
    }

    #[test]
    fn test_proxy_transparent_should_always_proxy() {
        let store = RecordingStore::new(ProxyMode::ProxyTransparent);
        let sig = RequestSignature::new("GET", "/test", None, &[]);

        // Transparent mode always proxies
        assert!(store.should_proxy(&sig));
    }

    #[test]
    fn test_mode_accessor() {
        let once = RecordingStore::new(ProxyMode::ProxyOnce);
        assert_eq!(once.mode(), ProxyMode::ProxyOnce);

        let always = RecordingStore::new(ProxyMode::ProxyAlways);
        assert_eq!(always.mode(), ProxyMode::ProxyAlways);

        let transparent = RecordingStore::new(ProxyMode::ProxyTransparent);
        assert_eq!(transparent.mode(), ProxyMode::ProxyTransparent);
    }
}

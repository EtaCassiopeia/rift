//! Debug/preview inspection and response-mode (rift/proxy/inject) dispatch helpers.
//!
//! Part of the `Imposter` implementation; see `core/mod.rs` for the struct definition.

use super::*;

impl Imposter {
    /// Get all stubs info for debug purposes (Rift extension)
    pub fn get_all_stubs_info(&self) -> Vec<DebugStubInfo> {
        let stubs = self.stubs.read();
        stubs
            .iter()
            .map(|stub_state| &stub_state.stub)
            .enumerate()
            .map(|(index, stub)| DebugStubInfo {
                index,
                id: stub.id.clone(),
                predicates: stub.predicates.clone(),
                response_count: stub.responses.len(),
            })
            .collect()
    }

    /// Get imposter info for debug purposes (Rift extension)
    pub fn get_debug_imposter_info(&self) -> DebugImposter {
        let stubs = self.stubs.read();
        DebugImposter {
            port: self.config.port.unwrap_or(0),
            name: self.config.name.clone(),
            protocol: self.config.protocol.clone(),
            stub_count: stubs.len(),
        }
    }

    /// Create response preview from a stub (Rift extension)
    pub fn get_response_preview(&self, stub_state: &StubState) -> DebugResponsePreview {
        if stub_state.stub.responses.is_empty() {
            return DebugResponsePreview {
                response_type: "unknown".to_string(),
                status_code: None,
                headers: None,
                body_preview: None,
            };
        }

        // Get the current response from the cycler
        if let Some(response) = stub_state.peek_response() {
            return create_response_preview(response);
        }

        DebugResponsePreview {
            response_type: "unknown".to_string(),
            status_code: None,
            headers: None,
            body_preview: None,
        }
    }

    /// Convert hyper HeaderMap to HashMap<String, String>
    /// Uses Title-Case for header keys to match Mountebank's convention.
    pub(crate) fn header_map_to_hashmap(headers: &hyper::HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .map(|(k, v)| {
                (
                    crate::behaviors::header_to_title_case(k.as_str()),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }

    /// Execute a stub and get the response with behaviors and rift extensions
    /// Returns (status, headers, body, behaviors, rift_extension, response_mode, is_fault)
    #[allow(clippy::type_complexity)]
    pub fn execute_stub_with_rift(
        &self,
        stub_state: &StubState,
    ) -> Option<(
        u16,
        HashMap<String, Vec<String>>,
        String,
        Option<serde_json::Value>,
        Option<RiftResponseExtension>,
        ResponseMode,
        bool,
    )> {
        let response = stub_state.get_next_response()?;
        execute_stub_response_with_rift(response)
    }

    /// Get RiftScript response if present
    /// Note: This peeks at the current response without advancing the cycler
    pub fn get_rift_script_response(&self, stub_state: &StubState) -> Option<RiftScriptConfig> {
        let response = stub_state.peek_response()?;
        get_rift_script_config(response)
    }

    /// Advance cycler for RiftScript response
    pub fn advance_cycler_for_rift_script(&self, stub_state: &StubState) {
        // Just cycling as a side effect
        _ = stub_state.get_next_response();
    }

    /// Check if a stub response is a proxy and return the proxy config
    /// Note: This peeks at the current response without advancing the cycler
    pub fn get_proxy_response(&self, stub: &StubState) -> Option<ProxyResponse> {
        let response = stub.peek_response()?;

        match response {
            StubResponse::Proxy { proxy } => Some(proxy.clone()),
            _ => None,
        }
    }

    /// Advance the response cycler for a proxy response
    /// This should be called after successfully handling a proxy response
    pub fn advance_cycler_for_proxy(&self, stub_state: &StubState) {
        // Assume proxies won't have a repeat count anyway, so a normal advance works.
        _ = stub_state.get_next_response();
    }

    /// Check if a stub response is an inject and return the inject function
    /// Note: This peeks at the current response without advancing the cycler
    // Used with javascript feature
    pub fn get_inject_response(&self, stub_state: &StubState) -> Option<String> {
        let response = stub_state.peek_response()?;
        match response {
            StubResponse::Inject { inject } => Some(inject.clone()),
            _ => None,
        }
    }

    /// Advance the response cycler for an inject response
    /// This should be called after successfully handling an inject response
    // Used with javascript feature
    pub fn advance_cycler_for_inject(&self, stub_state: &StubState) {
        _ = stub_state.get_next_response();
    }
}

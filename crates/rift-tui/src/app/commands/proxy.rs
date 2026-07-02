//! Proxy-related commands for App

use super::super::*;

impl App {
    /// Show create proxy imposter dialog
    pub fn show_create_proxy_imposter(&mut self) {
        self.input_state = InputState {
            port: String::new(),
            name: String::new(),
            protocol: "http".to_string(),
            target_url: String::new(),
            proxy_mode: 0, // proxyOnce
            focus_field: 0,
            file_path: String::new(),
            cursor_pos: 0,
        };
        self.overlay = Overlay::Input {
            prompt: "Create Proxy Imposter".to_string(),
            action: InputAction::CreateProxyImposter,
        };
    }

    /// Create a proxy imposter for recording
    pub async fn create_proxy_imposter(&mut self) {
        if self.input_state.target_url.is_empty() {
            self.set_status("Target URL is required".to_string(), StatusLevel::Error);
            return;
        }

        let port = if self.input_state.port.is_empty() {
            None
        } else {
            match self.input_state.port.parse::<u16>() {
                Ok(p) => Some(p),
                Err(_) => {
                    self.set_status("Invalid port number".to_string(), StatusLevel::Error);
                    return;
                }
            }
        };

        let name = if self.input_state.name.is_empty() {
            None
        } else {
            Some(self.input_state.name.clone())
        };

        self.is_loading = true;
        match self
            .client
            .create_proxy_imposter(
                port,
                name,
                &self.input_state.target_url,
                self.input_state.proxy_mode_str(),
            )
            .await
        {
            Ok(port) => {
                self.set_status(
                    format!("Created proxy imposter :{port}"),
                    StatusLevel::Success,
                );
                self.overlay = Overlay::None;
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to create: {e}"), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }

    /// Confirm apply recorded stubs
    pub fn confirm_apply_recorded_stubs(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => *port,
            _ => return,
        };

        self.overlay = Overlay::Confirm {
            message: format!(
                "Apply recorded stubs to :{port}?\nThis will remove proxy stubs and replace with recorded responses."
            ),
            action: PendingAction::ApplyRecordedStubs { port },
        };
    }

    /// Apply recorded stubs (remove proxy, keep recorded responses)
    pub async fn apply_recorded_stubs(&mut self, port: u16) {
        self.is_loading = true;

        // Get the imposter config with removeProxies=true
        match self.client.export_imposter(port, true).await {
            Ok(json) => {
                // Parse and re-apply
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(mut config) => {
                        // Turn off recording since we're no longer proxying
                        if let Some(obj) = config.as_object_mut() {
                            obj.insert(
                                "recordRequests".to_string(),
                                serde_json::Value::Bool(false),
                            );
                        }

                        // Delete and recreate imposter with new stubs
                        if let Err(e) = self.client.delete_imposter(port).await {
                            self.set_status(format!("Failed to apply: {e}"), StatusLevel::Error);
                            self.is_loading = false;
                            self.overlay = Overlay::None;
                            return;
                        }

                        // Recreate with the filtered config
                        let url = format!("{}/imposters", self.client.base_url());
                        let resp = self.client.client().post(url).json(&config).send().await;

                        match resp {
                            Ok(r) if r.status().is_success() => {
                                self.set_status(
                                    format!("Applied recorded stubs to :{port}"),
                                    StatusLevel::Success,
                                );
                                self.refresh().await;
                            }
                            Ok(r) => {
                                self.set_status(
                                    format!("Failed to apply: HTTP {}", r.status()),
                                    StatusLevel::Error,
                                );
                            }
                            Err(e) => {
                                self.set_status(
                                    format!("Failed to apply: {e}"),
                                    StatusLevel::Error,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to parse config: {e}"), StatusLevel::Error);
                    }
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to export: {e}"), StatusLevel::Error);
            }
        }

        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Confirm clear proxy responses
    pub fn confirm_clear_proxy_responses(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            self.overlay = Overlay::Confirm {
                message: format!("Clear all proxy recordings for :{port}?"),
                action: PendingAction::ClearProxyResponses { port },
            };
        }
    }

    /// Clear proxy responses
    pub async fn clear_proxy_responses(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.clear_proxy_responses(port).await {
            Ok(_) => {
                self.set_status("Cleared proxy recordings".to_string(), StatusLevel::Success);
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to clear: {e}"), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Confirm clear requests
    pub fn confirm_clear_requests(&mut self) {
        let port = match &self.view {
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            self.overlay = Overlay::Confirm {
                message: format!("Clear all recorded requests for :{port}?"),
                action: PendingAction::ClearRequests { port },
            };
        }
    }

    /// Clear recorded requests
    pub async fn clear_requests(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.clear_requests(port).await {
            Ok(_) => {
                self.set_status(
                    "Cleared recorded requests".to_string(),
                    StatusLevel::Success,
                );
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to clear: {e}"), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }
}

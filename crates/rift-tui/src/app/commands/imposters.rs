//! Imposter management commands for App

use super::super::*;

impl App {
    /// Enter imposter detail view
    pub async fn enter_imposter_detail(&mut self) {
        if let Some(imp) = self.selected_imposter() {
            let port = imp.port;
            self.is_loading = true;
            match self.client.get_imposter(port).await {
                Ok(detail) => {
                    self.current_imposter = Some(detail);
                    self.stub_list_state.select(Some(0));
                    self.navigate(View::ImposterDetail { port });
                }
                _ => {
                    self.set_status(
                        format!("Failed to load imposter :{}", port),
                        StatusLevel::Error,
                    );
                }
            }
            self.is_loading = false;
        }
    }

    /// Toggle enable/disable for selected imposter
    pub async fn toggle_imposter(&mut self) {
        let port = match &self.view {
            View::ImposterList => self.selected_imposter().map(|i| i.port),
            View::ImposterDetail { port } => Some(*port),
            _ => None,
        };

        if let Some(port) = port {
            // Find if enabled
            let enabled = self
                .imposters
                .iter()
                .find(|i| i.port == port)
                .map(|i| i.enabled);

            let result = if enabled.unwrap_or(true) {
                self.client.disable_imposter(port).await
            } else {
                self.client.enable_imposter(port).await
            };

            match result {
                Ok(_) => {
                    let action = if enabled.unwrap_or(true) {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    self.set_status(
                        format!("Imposter :{} {}", port, action),
                        StatusLevel::Success,
                    );
                    self.refresh().await;
                }
                Err(e) => {
                    self.set_status(
                        format!("Failed to toggle imposter: {}", e),
                        StatusLevel::Error,
                    );
                }
            }
        }
    }

    /// Show delete confirmation
    pub fn confirm_delete_imposter(&mut self) {
        if let Some(imp) = self.selected_imposter() {
            self.overlay = Overlay::Confirm {
                message: format!(
                    "Delete imposter :{}{}?",
                    imp.port,
                    imp.name
                        .as_ref()
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                ),
                action: PendingAction::DeleteImposter { port: imp.port },
            };
        }
    }

    /// Delete an imposter
    pub async fn delete_imposter(&mut self, port: u16) {
        self.is_loading = true;
        match self.client.delete_imposter(port).await {
            Ok(_) => {
                self.set_status(format!("Deleted imposter :{}", port), StatusLevel::Success);
                self.refresh().await;
                if matches!(self.view, View::ImposterDetail { port: p } if p == port) {
                    self.view = View::ImposterList;
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to delete: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    /// Show create imposter dialog
    pub fn show_create_imposter(&mut self) {
        self.input_state = InputState {
            port: String::new(),
            name: String::new(),
            protocol: "http".to_string(),
            target_url: String::new(),
            proxy_mode: 0,
            focus_field: 0,
            file_path: String::new(),
            cursor_pos: 0,
        };
        self.overlay = Overlay::Input {
            prompt: "Create New Imposter".to_string(),
            action: InputAction::CreateImposter,
        };
    }

    /// Create a new imposter
    pub async fn create_imposter(&mut self) {
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

        let request = CreateImposterRequest {
            port,
            protocol: self.input_state.protocol.clone(),
            name: if self.input_state.name.is_empty() {
                None
            } else {
                Some(self.input_state.name.clone())
            },
            record_requests: false,
            stubs: Vec::new(),
        };

        self.is_loading = true;
        match self.client.create_imposter(request).await {
            Ok(port) => {
                self.set_status(format!("Created imposter :{}", port), StatusLevel::Success);
                self.overlay = Overlay::None;
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to create: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }
}

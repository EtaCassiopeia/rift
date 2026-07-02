//! I/O commands (import/export/clipboard/save) for App

use super::super::*;

impl App {
    /// Export imposter config
    pub async fn export_imposter(&mut self, remove_proxies: bool) {
        let port = match &self.view {
            View::ImposterDetail { port } => *port,
            _ => return,
        };

        self.is_loading = true;
        match self.client.export_imposter(port, remove_proxies).await {
            Ok(json) => {
                let title = if remove_proxies {
                    format!(
                        "Exported Stubs (Port :{}) - [s]ave [c]opy [A]pply [Esc]close",
                        port
                    )
                } else {
                    format!(
                        "Exported Config (Port :{}) - [s]ave [c]opy [Esc]close",
                        port
                    )
                };
                self.overlay = Overlay::Export {
                    title,
                    content: json,
                    port: Some(port),
                };
            }
            Err(e) => {
                self.set_status(format!("Failed to export: {}", e), StatusLevel::Error);
            }
        }
        self.is_loading = false;
    }

    /// Copy content to clipboard
    pub fn copy_to_clipboard(&mut self, content: &str) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(content.to_string()) {
                    self.set_status(format!("Failed to copy: {}", e), StatusLevel::Error);
                } else {
                    self.set_status("Copied to clipboard".to_string(), StatusLevel::Success);
                }
            }
            Err(e) => {
                self.set_status(
                    format!("Clipboard not available: {}", e),
                    StatusLevel::Error,
                );
            }
        }
    }

    /// Paste from clipboard, returning the text if successful
    pub(in super::super) fn paste_from_clipboard(&self) -> Option<String> {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
    }

    /// Show save file dialog
    pub fn show_save_dialog(&mut self, content: String, port: u16) {
        // Generate default filename
        let default_path = dirs::home_dir()
            .map(|h| h.join(format!("imposter-{}.json", port)))
            .unwrap_or_else(|| std::path::PathBuf::from(format!("imposter-{}.json", port)));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: format!("Save imposter :{} to file", port),
            action: FileAction::SaveExport { content, port },
        };
    }

    /// Expand tilde in path to home directory
    pub(super) fn expand_path(path: &str) -> String {
        if let Some(rest) = path.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(rest).to_string_lossy().to_string();
            }
        } else if path == "~"
            && let Some(home) = dirs::home_dir()
        {
            return home.to_string_lossy().to_string();
        }
        path.to_string()
    }

    /// Save content to file
    pub fn save_to_file(&mut self, path: &str, content: &str) {
        let expanded_path = Self::expand_path(path);
        match std::fs::write(&expanded_path, content) {
            Ok(_) => {
                self.set_status(format!("Saved to {}", expanded_path), StatusLevel::Success);
                self.overlay = Overlay::None;
            }
            Err(e) => {
                self.set_status(format!("Failed to save: {}", e), StatusLevel::Error);
            }
        }
    }

    /// Show import file dialog
    pub fn show_import_file_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        self.input_state.cursor_pos = default_path.len();
        self.input_state.file_path = default_path;
        self.overlay = Overlay::FilePathInput {
            prompt: "Import imposter from JSON file".to_string(),
            action: FileAction::ImportFile,
        };
    }

    /// Show import folder dialog
    pub fn show_import_folder_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        self.input_state.cursor_pos = default_path.len();
        self.input_state.file_path = default_path;
        self.overlay = Overlay::FilePathInput {
            prompt: "Import imposters from folder (*.json)".to_string(),
            action: FileAction::ImportFolder,
        };
    }

    /// Import imposter from file with validation
    pub async fn import_from_file(&mut self, path: &str) {
        self.is_loading = true;
        let expanded_path = Self::expand_path(path);

        match std::fs::read_to_string(&expanded_path) {
            Ok(content) => {
                // Validate the content before importing
                let report = validate_imposter_json(&content, &expanded_path);

                if report.has_errors() {
                    // Block import on errors - show validation results
                    self.validation_scroll_offset = 0;
                    self.overlay = Overlay::ValidationResult {
                        report,
                        action: ValidationAction::EditorInfo, // Can't proceed with errors
                    };
                    self.is_loading = false;
                    return;
                }

                if report.has_warnings() {
                    // Show warnings but allow proceeding
                    self.validation_scroll_offset = 0;
                    self.overlay = Overlay::ValidationResult {
                        report,
                        action: ValidationAction::ProceedWithImport {
                            path: expanded_path.clone(),
                            content: content.clone(),
                        },
                    };
                    self.is_loading = false;
                    return;
                }

                // No issues - proceed with import
                self.do_import(&content).await;
            }
            Err(e) => {
                self.set_status(format!("Failed to read file: {}", e), StatusLevel::Error);
            }
        }

        self.is_loading = false;
    }

    /// Actually perform the import (called after validation passes or user confirms)
    pub async fn do_import(&mut self, content: &str) {
        match serde_json::from_str::<serde_json::Value>(content) {
            Ok(config) => {
                let url = format!("{}/imposters", self.client.base_url());
                let resp = self.client.client().post(url).json(&config).send().await;

                match resp {
                    Ok(r) if r.status().is_success() => {
                        self.set_status("Import successful".to_string(), StatusLevel::Success);
                        self.overlay = Overlay::None;
                        self.refresh().await;
                    }
                    Ok(r) => {
                        let body = r.text().await.unwrap_or_default();
                        self.set_status(format!("Failed to import: {}", body), StatusLevel::Error);
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to import: {}", e), StatusLevel::Error);
                    }
                }
            }
            Err(e) => {
                self.set_status(format!("Invalid JSON: {}", e), StatusLevel::Error);
            }
        }
    }

    /// Import imposters from folder
    pub async fn import_from_folder(&mut self, folder: &str) {
        self.is_loading = true;
        let expanded_folder = Self::expand_path(folder);

        let path = std::path::Path::new(&expanded_folder);
        if !path.is_dir() {
            self.set_status(
                format!("{} is not a directory", expanded_folder),
                StatusLevel::Error,
            );
            self.is_loading = false;
            return;
        }

        let mut imported = 0;
        let mut failed = 0;

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().map(|e| e == "json").unwrap_or(false)
                    && let Ok(content) = std::fs::read_to_string(&file_path)
                {
                    if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                        let url = format!("{}/imposters", self.client.base_url());
                        let resp = self.client.client().post(url).json(&config).send().await;

                        if resp.map(|r| r.status().is_success()).unwrap_or(false) {
                            imported += 1;
                        } else {
                            failed += 1;
                        }
                    } else {
                        failed += 1;
                    }
                }
            }
        }

        if failed > 0 {
            self.set_status(
                format!("Imported {} imposters, {} failed", imported, failed),
                StatusLevel::Warning,
            );
        } else {
            self.set_status(
                format!("Imported {} imposters", imported),
                StatusLevel::Success,
            );
        }

        self.overlay = Overlay::None;
        self.refresh().await;
        self.is_loading = false;
    }

    /// Show export all dialog
    pub fn show_export_all_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.join("imposters.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("imposters.json"));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: "Export all imposters to file".to_string(),
            action: FileAction::ExportAll,
        };
    }

    /// Show export to folder dialog
    pub fn show_export_folder_dialog(&mut self) {
        let default_path = dirs::home_dir()
            .map(|h| h.join("imposters"))
            .unwrap_or_else(|| std::path::PathBuf::from("imposters"));

        let path_str = default_path.to_string_lossy().to_string();
        self.input_state.cursor_pos = path_str.len();
        self.input_state.file_path = path_str;
        self.overlay = Overlay::FilePathInput {
            prompt: "Export imposters to folder (one file per imposter)".to_string(),
            action: FileAction::ExportToFolder,
        };
    }

    /// Export all imposters to a single file
    pub async fn export_all_to_file(&mut self, path: &str) {
        self.is_loading = true;
        let expanded_path = Self::expand_path(path);

        match self.client.export_all_imposters().await {
            Ok(json) => match std::fs::write(&expanded_path, &json) {
                Ok(_) => {
                    self.set_status(
                        format!("Exported to {}", expanded_path),
                        StatusLevel::Success,
                    );
                    self.overlay = Overlay::None;
                }
                Err(e) => {
                    self.set_status(format!("Failed to write: {}", e), StatusLevel::Error);
                }
            },
            Err(e) => {
                self.set_status(format!("Failed to export: {}", e), StatusLevel::Error);
            }
        }

        self.is_loading = false;
    }

    /// Export imposters to individual files in a folder
    pub async fn export_to_folder(&mut self, folder: &str) {
        self.is_loading = true;
        let expanded_folder = Self::expand_path(folder);

        let path = std::path::Path::new(&expanded_folder);

        // Create folder if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(path) {
            self.set_status(
                format!("Failed to create folder: {}", e),
                StatusLevel::Error,
            );
            self.is_loading = false;
            return;
        }

        let mut exported = 0;
        let mut failed = 0;

        for imp in &self.imposters {
            match self.client.export_imposter(imp.port, false).await {
                Ok(json) => {
                    let filename = if let Some(name) = &imp.name {
                        format!("{}-{}.json", imp.port, name.replace(['/', '\\', ' '], "_"))
                    } else {
                        format!("{}.json", imp.port)
                    };
                    let file_path = path.join(filename);
                    if std::fs::write(&file_path, &json).is_ok() {
                        exported += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        if failed > 0 {
            self.set_status(
                format!("Exported {} imposters, {} failed", exported, failed),
                StatusLevel::Warning,
            );
        } else {
            self.set_status(
                format!("Exported {} imposters to {}", exported, folder),
                StatusLevel::Success,
            );
        }

        self.overlay = Overlay::None;
        self.is_loading = false;
    }
}

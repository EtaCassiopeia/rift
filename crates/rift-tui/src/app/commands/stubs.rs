//! Stub management commands for App

use super::super::*;

impl App {
    /// Start editing a stub
    pub fn start_stub_edit(&mut self) {
        let (port, idx) = match &self.view {
            View::ImposterDetail { port } => {
                if let Some(idx) = self.stub_list_state.selected() {
                    (*port, idx)
                } else {
                    return;
                }
            }
            View::StubDetail { port, index } => (*port, *index),
            _ => return,
        };

        if let Some(imp) = &self.current_imposter
            && let Some(stub) = imp.stubs.get(idx)
        {
            let json = serde_json::to_string_pretty(stub).unwrap_or_default();
            self.stub_editor = Some(StubEditor::new(&json));
            self.navigate(View::StubEdit {
                port,
                index: Some(idx),
            });
        }
    }

    /// Start creating a new stub
    pub fn start_stub_create(&mut self) {
        if let View::ImposterDetail { port } = self.view {
            let template = r#"{
  "predicates": [
    {"equals": {"method": "GET", "path": "/example"}}
  ],
  "responses": [
    {"is": {"statusCode": 200, "body": "Hello, World!"}}
  ]
}"#;
            self.stub_editor = Some(StubEditor::new(template));
            self.navigate(View::StubEdit { port, index: None });
        }
    }

    /// Save the current stub being edited
    pub async fn save_stub(&mut self) {
        if let Some(editor) = &mut self.stub_editor {
            if !editor.validate() {
                return;
            }

            if let Some(stub) = editor.get_stub()
                && let View::StubEdit { port, index } = self.view
            {
                self.is_loading = true;
                let result = if let Some(idx) = index {
                    self.client.update_stub(port, idx, stub).await
                } else {
                    self.client.add_stub(port, stub, None).await
                };

                match result {
                    Ok(_) => {
                        self.set_status("Stub saved".to_string(), StatusLevel::Success);
                        self.stub_editor = None;
                        self.go_back();
                        self.refresh().await;
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to save: {e}"), StatusLevel::Error);
                    }
                }
                self.is_loading = false;
            }
        }
    }

    /// Cancel stub editing
    pub fn cancel_stub_edit(&mut self) {
        self.stub_editor = None;
        self.go_back();
    }

    /// Show validation results for the current editor content
    pub fn show_editor_validation(&mut self) {
        if let Some(editor) = &self.stub_editor {
            if let Some(report) = &editor.validation_report {
                if report.has_issues() {
                    self.validation_scroll_offset = 0;
                    self.overlay = Overlay::ValidationResult {
                        report: report.clone(),
                        action: ValidationAction::EditorInfo,
                    };
                } else {
                    self.set_status(
                        "No validation issues found".to_string(),
                        StatusLevel::Success,
                    );
                }
            } else {
                self.set_status(
                    "Run validation first (edit the content)".to_string(),
                    StatusLevel::Info,
                );
            }
        }
    }

    /// Confirm delete stub
    pub fn confirm_delete_stub(&mut self) {
        if let View::ImposterDetail { port } = self.view
            && let Some(idx) = self.stub_list_state.selected()
        {
            self.overlay = Overlay::Confirm {
                message: format!("Delete stub #{idx} from :{port}?"),
                action: PendingAction::DeleteStub { port, index: idx },
            };
        }
    }

    /// Delete a stub
    pub async fn delete_stub(&mut self, port: u16, index: usize) {
        self.is_loading = true;
        match self.client.delete_stub(port, index).await {
            Ok(_) => {
                self.set_status("Stub deleted".to_string(), StatusLevel::Success);
                self.refresh().await;
            }
            Err(e) => {
                self.set_status(format!("Failed to delete: {e}"), StatusLevel::Error);
            }
        }
        self.is_loading = false;
        self.overlay = Overlay::None;
    }

    pub(in super::super) async fn reorder_stub(&mut self, direction: i32) {
        let View::ImposterDetail { port } = self.view else {
            return;
        };
        if let Some(idx) = self.stub_list_state.selected() {
            let new_idx = idx as i32 + direction;
            if new_idx < 0 {
                return;
            }
            let new_idx = new_idx as usize;
            if let Some(imp) = &mut self.current_imposter {
                if new_idx >= imp.stubs.len() {
                    return;
                }
                imp.stubs.swap(idx, new_idx);
                let stubs = imp.stubs.clone();
                match self.client.update_stubs(port, stubs).await {
                    Ok(_) => {
                        self.stub_list_state.select(Some(new_idx));
                        self.set_status(
                            format!("Moved stub to #{}", new_idx + 1),
                            StatusLevel::Success,
                        );
                        self.refresh().await;
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to reorder: {e}"), StatusLevel::Error);
                        // Refresh to restore correct order
                        self.refresh().await;
                    }
                }
            }
        }
    }

    pub(in super::super) async fn duplicate_stub(&mut self) {
        let port = match self.view {
            View::ImposterDetail { port } => port,
            View::StubDetail { port, .. } => port,
            _ => return,
        };
        let idx = match self.view {
            View::StubDetail { index, .. } => Some(index),
            View::ImposterDetail { .. } => self.stub_list_state.selected(),
            _ => None,
        };
        if let Some(idx) = idx
            && let Some(stub) = self
                .current_imposter
                .as_ref()
                .and_then(|imp| imp.stubs.get(idx))
                .cloned()
        {
            self.is_loading = true;
            match self.client.add_stub(port, stub, None).await {
                Ok(_) => {
                    self.set_status("Stub duplicated".to_string(), StatusLevel::Success);
                    self.refresh().await;
                }
                Err(e) => self.set_status(format!("Failed to duplicate: {e}"), StatusLevel::Error),
            }
            self.is_loading = false;
        }
    }
}

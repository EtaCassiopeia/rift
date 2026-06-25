//! Search and filtering methods for App

use super::*;

impl App {
    /// Move selection up in current list (skips filtered items when search is active)
    pub fn select_previous(&mut self) {
        match &self.view {
            View::ImposterList => {
                if self.imposters.is_empty() {
                    return;
                }

                // Get indices of matching imposters
                let matching_indices: Vec<usize> = if self.search_query.is_empty() {
                    (0..self.imposters.len()).collect()
                } else {
                    self.imposters
                        .iter()
                        .enumerate()
                        .filter(|(_, imp)| self.imposter_matches_search(imp))
                        .map(|(i, _)| i)
                        .collect()
                };

                if matching_indices.is_empty() {
                    return;
                }

                let current = self.imposter_list_state.selected().unwrap_or(0);
                // Find the previous matching index
                let next = matching_indices
                    .iter()
                    .rev()
                    .find(|&&i| i < current)
                    .or_else(|| matching_indices.last())
                    .copied()
                    .unwrap_or(current);

                self.imposter_list_state.select(Some(next));
            }
            View::ImposterDetail { .. } | View::StubDetail { .. } => {
                if self.focus == FocusArea::Left {
                    if let Some(imp) = &self.current_imposter {
                        if imp.stubs.is_empty() {
                            return;
                        }

                        // Get indices of matching stubs
                        let matching_indices = self.filtered_stubs();

                        if matching_indices.is_empty() {
                            return;
                        }

                        let current = self.stub_list_state.selected().unwrap_or(0);
                        let next = matching_indices
                            .iter()
                            .rev()
                            .find(|&&i| i < current)
                            .or_else(|| matching_indices.last())
                            .copied()
                            .unwrap_or(current);

                        self.stub_list_state.select(Some(next));
                    }
                } else if let Some(imp) = &self.current_imposter {
                    let len = imp.requests.len();
                    if len > 0 {
                        let i = self
                            .request_list_state
                            .selected()
                            .map(|i| if i == 0 { len - 1 } else { i - 1 })
                            .unwrap_or(0);
                        self.request_list_state.select(Some(i));
                    }
                }
            }
            _ => {}
        }
    }

    /// Move selection down in current list (skips filtered items when search is active)
    pub fn select_next(&mut self) {
        match &self.view {
            View::ImposterList => {
                if self.imposters.is_empty() {
                    return;
                }

                // Get indices of matching imposters
                let matching_indices: Vec<usize> = if self.search_query.is_empty() {
                    (0..self.imposters.len()).collect()
                } else {
                    self.imposters
                        .iter()
                        .enumerate()
                        .filter(|(_, imp)| self.imposter_matches_search(imp))
                        .map(|(i, _)| i)
                        .collect()
                };

                if matching_indices.is_empty() {
                    return;
                }

                let current = self.imposter_list_state.selected().unwrap_or(0);
                // Find the next matching index
                let next = matching_indices
                    .iter()
                    .find(|&&i| i > current)
                    .or_else(|| matching_indices.first())
                    .copied()
                    .unwrap_or(current);

                self.imposter_list_state.select(Some(next));
            }
            View::ImposterDetail { .. } | View::StubDetail { .. } => {
                if self.focus == FocusArea::Left {
                    if let Some(imp) = &self.current_imposter {
                        if imp.stubs.is_empty() {
                            return;
                        }

                        // Get indices of matching stubs
                        let matching_indices = self.filtered_stubs();

                        if matching_indices.is_empty() {
                            return;
                        }

                        let current = self.stub_list_state.selected().unwrap_or(0);
                        let next = matching_indices
                            .iter()
                            .find(|&&i| i > current)
                            .or_else(|| matching_indices.first())
                            .copied()
                            .unwrap_or(current);

                        self.stub_list_state.select(Some(next));
                    }
                } else if let Some(imp) = &self.current_imposter {
                    let len = imp.requests.len();
                    if len > 0 {
                        let i = self
                            .request_list_state
                            .selected()
                            .map(|i| (i + 1) % len)
                            .unwrap_or(0);
                        self.request_list_state.select(Some(i));
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle search input
    pub(super) fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_active = false;
                self.search_query.clear();
            }
            KeyCode::Enter => {
                self.search_active = false;
                // Keep the query for filtering, select first match
                if !self.search_query.is_empty() {
                    self.select_first_match();
                }
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.push(c);
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(text) = self.paste_from_clipboard() {
                    self.search_query.push_str(&text);
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.clear();
            }
            _ => {}
        }
    }

    /// Select the first matching item after search
    fn select_first_match(&mut self) {
        match &self.view {
            View::ImposterList => {
                let filtered = self.filtered_imposters();
                if !filtered.is_empty() {
                    // Find the index of the first matching imposter in the original list
                    if let Some(first) = filtered.first() {
                        if let Some(idx) = self.imposters.iter().position(|i| i.port == first.port)
                        {
                            self.imposter_list_state.select(Some(idx));
                        }
                    }
                }
            }
            View::ImposterDetail { .. } => {
                let filtered = self.filtered_stubs();
                if !filtered.is_empty() {
                    self.stub_list_state.select(Some(filtered[0]));
                }
            }
            _ => {}
        }
    }

    /// Get filtered imposters based on search query
    pub fn filtered_imposters(&self) -> Vec<&ImposterSummary> {
        if self.search_query.is_empty() {
            self.imposters.iter().collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.imposters
                .iter()
                .filter(|imp| {
                    imp.port.to_string().contains(&query)
                        || imp
                            .name
                            .as_ref()
                            .map(|n| n.to_lowercase().contains(&query))
                            .unwrap_or(false)
                        || imp.protocol.to_lowercase().contains(&query)
                })
                .collect()
        }
    }

    /// Get filtered stub indices based on search query
    pub fn filtered_stubs(&self) -> Vec<usize> {
        if self.search_query.is_empty() {
            if let Some(imp) = &self.current_imposter {
                (0..imp.stubs.len()).collect()
            } else {
                vec![]
            }
        } else {
            let query = self.search_query.to_lowercase();
            if let Some(imp) = &self.current_imposter {
                imp.stubs
                    .iter()
                    .enumerate()
                    .filter(|(_, stub)| {
                        // Match scenario name
                        stub.scenario_name.as_ref().map(|n| n.to_lowercase().contains(&query)).unwrap_or(false)
                            // Match predicates (path, method)
                            || stub.predicates.iter().any(|p| {
                                p.to_string().to_lowercase().contains(&query)
                            })
                            // Match response type
                            || stub.responses.iter().any(|r| {
                                r.to_string().to_lowercase().contains(&query)
                            })
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                vec![]
            }
        }
    }

    /// Check if an imposter matches the current search
    pub fn imposter_matches_search(&self, imp: &ImposterSummary) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        let query = self.search_query.to_lowercase();
        imp.port.to_string().contains(&query)
            || imp
                .name
                .as_ref()
                .map(|n| n.to_lowercase().contains(&query))
                .unwrap_or(false)
            || imp.protocol.to_lowercase().contains(&query)
    }

    /// Check if a stub index matches the current search
    pub fn stub_matches_search(&self, index: usize) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        self.filtered_stubs().contains(&index)
    }

    /// Clear search
    pub fn clear_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }
}

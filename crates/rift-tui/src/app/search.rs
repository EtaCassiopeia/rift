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
                    if let Some(first) = filtered.first()
                        && let Some(idx) = self.imposters.iter().position(|i| i.port == first.port)
                    {
                        self.imposter_list_state.select(Some(idx));
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

#[cfg(test)]
mod tests {
    use crate::api::ImposterDetail;
    use crate::app::tests::{make_imposter, make_test_app};

    // ─── filtered_imposters ───────────────────────────────────────────────────

    #[test]
    fn test_filtered_imposters_no_query_returns_all() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(4546, Some("api"), "http"),
        ];
        assert_eq!(app.filtered_imposters().len(), 2);
    }

    #[test]
    fn test_filtered_imposters_matches_by_port() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(9000, None, "http"),
        ];
        app.search_query = "9000".to_string();
        let filtered = app.filtered_imposters();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].port, 9000);
    }

    #[test]
    fn test_filtered_imposters_matches_by_name() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, Some("payment-service"), "http"),
            make_imposter(4546, Some("auth-service"), "http"),
        ];
        app.search_query = "auth".to_string();
        let filtered = app.filtered_imposters();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].port, 4546);
    }

    #[test]
    fn test_filtered_imposters_no_match_returns_empty() {
        let mut app = make_test_app();
        app.imposters = vec![make_imposter(4545, None, "http")];
        app.search_query = "zzznomatch".to_string();
        assert!(app.filtered_imposters().is_empty());
    }

    #[test]
    fn test_filtered_imposters_matches_by_protocol() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(4546, None, "tcp"),
        ];
        app.search_query = "tcp".to_string();
        let filtered = app.filtered_imposters();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].port, 4546);
    }

    // ─── filtered_stubs ───────────────────────────────────────────────────────

    #[test]
    fn test_filtered_stubs_no_imposter_returns_empty() {
        let app = make_test_app();
        assert!(app.filtered_stubs().is_empty());
    }

    fn make_detail_with_stubs(stubs: Vec<crate::api::Stub>) -> ImposterDetail {
        ImposterDetail {
            port: 4545,
            protocol: "http".to_string(),
            name: None,
            number_of_requests: 0,
            enabled: true,
            record_requests: false,
            stubs,
            requests: vec![],
        }
    }

    fn make_stub(scenario: Option<&str>) -> crate::api::Stub {
        crate::api::Stub {
            id: None,
            scenario_name: scenario.map(String::from),
            recorded_from: None,
            predicates: vec![],
            responses: vec![],
        }
    }

    #[test]
    fn test_filtered_stubs_no_query_returns_all_indices() {
        let mut app = make_test_app();
        app.current_imposter = Some(make_detail_with_stubs(vec![
            make_stub(None),
            make_stub(None),
        ]));
        let indices = app.filtered_stubs();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn test_filtered_stubs_matches_scenario_name() {
        let mut app = make_test_app();
        app.current_imposter = Some(make_detail_with_stubs(vec![
            make_stub(Some("payment-flow")),
            make_stub(Some("auth-flow")),
            make_stub(None),
        ]));
        app.search_query = "auth".to_string();
        let indices = app.filtered_stubs();
        assert_eq!(indices, vec![1], "only the auth-flow stub should match");
    }

    #[test]
    fn test_filtered_stubs_no_match_returns_empty() {
        let mut app = make_test_app();
        app.current_imposter = Some(make_detail_with_stubs(vec![make_stub(Some(
            "payment-flow",
        ))]));
        app.search_query = "zzznomatch".to_string();
        assert!(app.filtered_stubs().is_empty());
    }

    // ─── stub_matches_search ──────────────────────────────────────────────────

    #[test]
    fn test_stub_matches_search_empty_query_always_true() {
        let mut app = make_test_app();
        app.current_imposter = Some(make_detail_with_stubs(vec![make_stub(None)]));
        assert!(app.stub_matches_search(0));
    }

    #[test]
    fn test_stub_matches_search_scenario_name_hit() {
        let mut app = make_test_app();
        app.current_imposter = Some(make_detail_with_stubs(vec![
            make_stub(Some("payment-flow")),
            make_stub(None),
        ]));
        app.search_query = "payment".to_string();
        assert!(
            app.stub_matches_search(0),
            "index 0 (payment-flow) should match"
        );
        assert!(
            !app.stub_matches_search(1),
            "index 1 (no scenario) should not match"
        );
    }

    // ─── imposter_matches_search ──────────────────────────────────────────────

    #[test]
    fn test_imposter_matches_search_empty_query_always_true() {
        let app = make_test_app();
        let imp = make_imposter(4545, None, "http");
        assert!(app.imposter_matches_search(&imp));
    }

    #[test]
    fn test_imposter_matches_search_by_protocol() {
        let mut app = make_test_app();
        app.search_query = "tcp".to_string();
        let http_imp = make_imposter(4545, None, "http");
        let tcp_imp = make_imposter(4546, None, "tcp");
        assert!(!app.imposter_matches_search(&http_imp));
        assert!(app.imposter_matches_search(&tcp_imp));
    }

    // ─── select_next / select_previous ───────────────────────────────────────

    #[test]
    fn test_select_next_empty_list_does_not_panic() {
        let mut app = make_test_app();
        app.select_next(); // must not panic
    }

    #[test]
    fn test_select_next_advances_selection() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(4546, None, "http"),
            make_imposter(4547, None, "http"),
        ];
        app.imposter_list_state.select(Some(0));
        app.select_next();
        assert_eq!(app.imposter_list_state.selected(), Some(1));
    }

    #[test]
    fn test_select_next_wraps_to_first() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(4546, None, "http"),
        ];
        app.imposter_list_state.select(Some(1));
        app.select_next();
        assert_eq!(app.imposter_list_state.selected(), Some(0));
    }

    #[test]
    fn test_select_previous_wraps_to_last() {
        let mut app = make_test_app();
        app.imposters = vec![
            make_imposter(4545, None, "http"),
            make_imposter(4546, None, "http"),
        ];
        app.imposter_list_state.select(Some(0));
        app.select_previous();
        assert_eq!(app.imposter_list_state.selected(), Some(1));
    }

    #[test]
    fn test_clear_search_resets_state() {
        let mut app = make_test_app();
        app.search_active = true;
        app.search_query = "foo".to_string();
        app.clear_search();
        assert!(!app.search_active);
        assert!(app.search_query.is_empty());
    }
}

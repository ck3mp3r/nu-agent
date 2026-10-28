use std::collections::{HashMap, VecDeque};

use super::*;

#[derive(Debug, Clone, Default)]
pub struct McpSkillsState {
    pub(crate) mcp_servers: Vec<McpServerState>,
    pub(crate) mcp_panel_selection: usize,
    /// Type-to-filter query; empty means every row shows.
    pub(crate) mcp_panel_query: String,
    pub(crate) discoverable_skills: Vec<DiscoverableSkill>,
    pub(crate) skills_discovery_failed: bool,
    pub(crate) llm_visible_mcp_tool_count: usize,
    pub(crate) mcp_visible_tool_count_by_server: HashMap<String, usize>,
    pub(crate) mcp_visible_tool_names_by_server: HashMap<String, Vec<String>>,
    pub(crate) mcp_failure_reasons: HashMap<String, String>,
    pub(crate) pending_mcp_toggle_requests: VecDeque<McpToggleRequest>,
}

impl McpSkillsState {
    pub fn set_mcp_servers(&mut self, servers: Vec<McpServerState>) {
        self.mcp_servers = servers;
        self.mcp_visible_tool_count_by_server
            .retain(|name, _| self.mcp_servers.iter().any(|server| server.name == *name));
        self.mcp_visible_tool_names_by_server
            .retain(|name, _| self.mcp_servers.iter().any(|server| server.name == *name));
        self.mcp_failure_reasons.retain(|name, _| {
            self.mcp_servers.iter().any(|server| {
                server.name == *name && server.state == McpServerUsabilityState::Failed
            })
        });
        self.mcp_panel_query.clear();
        self.clamp_mcp_panel_selection();
    }

    /// The active type-to-filter query. Empty means every server row shows.
    pub fn mcp_panel_query(&self) -> &str {
        &self.mcp_panel_query
    }

    /// Server rows matching the active query, in server order.
    pub fn mcp_servers_filtered(&self) -> Vec<&McpServerState> {
        self.mcp_filtered_indices()
            .into_iter()
            .filter_map(|idx| self.mcp_servers.get(idx))
            .collect()
    }

    /// Append a filter character and re-filter from the top row.
    pub fn append_mcp_panel_query_char(&mut self, ch: char) {
        self.mcp_panel_query.push(ch);
        self.mcp_panel_selection = 0;
        self.clamp_mcp_panel_selection();
    }

    /// Drop the last filter character. Returns false when the query is already
    /// empty, so a caller can skip the repaint.
    pub fn backspace_mcp_panel_query_char(&mut self) -> bool {
        if self.mcp_panel_query.pop().is_none() {
            return false;
        }
        self.mcp_panel_selection = 0;
        self.clamp_mcp_panel_selection();
        true
    }

    pub fn clear_mcp_panel_query(&mut self) {
        self.mcp_panel_query.clear();
        self.clamp_mcp_panel_selection();
    }

    /// Clamp the selection into the filtered rows.
    pub fn clamp_mcp_panel_selection(&mut self) {
        let len = self.mcp_filtered_indices().len();
        if len == 0 {
            self.mcp_panel_selection = 0;
        } else if self.mcp_panel_selection >= len {
            self.mcp_panel_selection = len.saturating_sub(1);
        }
    }

    /// Indices of the server rows the query keeps, in server order.
    fn mcp_filtered_indices(&self) -> Vec<usize> {
        if self.mcp_panel_query.is_empty() {
            return (0..self.mcp_servers.len()).collect();
        }
        let q = self.mcp_panel_query.to_ascii_lowercase();
        self.mcp_servers
            .iter()
            .enumerate()
            .filter(|(_, server)| fuzzy_matches(&q, &server.name.to_ascii_lowercase()))
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Index into `mcp_servers` of the selected row of the filtered view.
    fn mcp_selected_server_index(&self) -> Option<usize> {
        self.mcp_filtered_indices()
            .get(self.mcp_panel_selection)
            .copied()
    }

    pub fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.llm_visible_mcp_tool_count = count;
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.mcp_visible_tool_count_by_server
            .insert(server_name.to_string(), count);
    }

    pub fn mcp_visible_tool_count_for_server_name(&self, server_name: &str) -> usize {
        self.mcp_visible_tool_count_by_server
            .get(server_name)
            .copied()
            .unwrap_or(0)
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        mut names: Vec<String>,
    ) {
        names.sort();
        names.dedup();
        self.mcp_visible_tool_names_by_server
            .insert(server_name.to_string(), names);
    }

    pub fn mcp_visible_tool_names_for_server_name(&self, server_name: &str) -> Vec<String> {
        self.mcp_visible_tool_names_by_server
            .get(server_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_discoverable_skills(&mut self, mut skills: Vec<DiscoverableSkill>) {
        skills.sort_by(|left, right| {
            left.source_priority
                .cmp(&right.source_priority)
                .then_with(|| {
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase())
                })
                .then_with(|| {
                    left.source
                        .to_ascii_lowercase()
                        .cmp(&right.source.to_ascii_lowercase())
                })
        });
        self.discoverable_skills = skills;
        self.skills_discovery_failed = false;
    }

    pub fn mark_skills_discovery_failed(&mut self) {
        self.discoverable_skills.clear();
        self.skills_discovery_failed = true;
    }

    pub fn discoverable_skills(&self) -> &[DiscoverableSkill] {
        &self.discoverable_skills
    }

    pub fn skills_discovery_failed(&self) -> bool {
        self.skills_discovery_failed
    }

    pub fn llm_visible_mcp_tool_count(&self) -> usize {
        self.llm_visible_mcp_tool_count
    }

    pub fn mcp_panel_move_up(&mut self) {
        let len = self.mcp_filtered_indices().len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = if self.mcp_panel_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.mcp_panel_selection.saturating_sub(1)
        };
    }

    pub fn mcp_panel_move_down(&mut self) {
        let len = self.mcp_filtered_indices().len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = (self.mcp_panel_selection + 1) % len;
    }

    /// Move the filtered selection one page up, clamped to the first row.
    pub fn mcp_panel_page_up(&mut self) {
        let len = self.mcp_filtered_indices().len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = self
            .mcp_panel_selection
            .saturating_sub(PANEL_PAGE_LINES)
            .min(len - 1);
    }

    /// Move the filtered selection one page down, clamped to the last row.
    pub fn mcp_panel_page_down(&mut self) {
        let len = self.mcp_filtered_indices().len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = self
            .mcp_panel_selection
            .saturating_add(PANEL_PAGE_LINES)
            .min(len - 1);
    }

    pub fn selected_mcp_server_name(&self) -> Option<&str> {
        self.selected_mcp_server()
            .map(|server| server.name.as_str())
    }

    pub fn selected_mcp_server_state(&self) -> Option<McpServerUsabilityState> {
        self.selected_mcp_server().map(|server| server.state)
    }

    /// The server row selected in the filtered view.
    pub fn selected_mcp_server(&self) -> Option<&McpServerState> {
        self.mcp_selected_server_index()
            .and_then(|idx| self.mcp_servers.get(idx))
    }

    pub fn set_mcp_server_state_by_name(
        &mut self,
        name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.set_mcp_server_state_by_name_with_reason(name, state, None)
    }

    pub fn set_mcp_server_state_by_name_with_reason(
        &mut self,
        name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
    ) -> bool {
        if let Some(server) = self
            .mcp_servers
            .iter_mut()
            .find(|server| server.name == name)
        {
            server.state = state;

            match state {
                McpServerUsabilityState::Failed => {
                    if let Some(reason) = reason {
                        let trimmed = reason.trim();
                        if !trimmed.is_empty() {
                            self.mcp_failure_reasons
                                .insert(name.to_string(), trimmed.to_string());
                        }
                    }
                }
                McpServerUsabilityState::Enabled | McpServerUsabilityState::Disabled => {
                    self.mcp_failure_reasons.remove(name);
                }
            }

            return true;
        }
        false
    }

    pub fn failed_mcp_servers_with_reasons(&self) -> Vec<(&str, Option<&str>)> {
        self.mcp_servers
            .iter()
            .filter(|server| server.state == McpServerUsabilityState::Failed)
            .map(|server| {
                (
                    server.name.as_str(),
                    self.mcp_failure_reasons
                        .get(server.name.as_str())
                        .map(String::as_str),
                )
            })
            .collect()
    }

    pub fn queue_selected_mcp_toggle_request(&mut self) -> bool {
        let Some(idx) = self.mcp_selected_server_index() else {
            return false;
        };
        let Some(server) = self.mcp_servers.get_mut(idx) else {
            return false;
        };

        log::debug!(
            "MCP toggle requested: server={} current_state={:?}",
            server.name,
            server.state
        );
        let request = match server.state {
            McpServerUsabilityState::Enabled => {
                server.state = McpServerUsabilityState::Disabled;
                McpToggleRequest {
                    server_name: server.name.clone(),
                    enable: false,
                }
            }
            McpServerUsabilityState::Disabled | McpServerUsabilityState::Failed => {
                McpToggleRequest {
                    server_name: server.name.clone(),
                    enable: true,
                }
            }
        };

        self.pending_mcp_toggle_requests.push_back(request);
        true
    }

    pub fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.pending_mcp_toggle_requests.pop_front()
    }

    pub fn mcp_counts(&self) -> (usize, usize, usize, usize) {
        let configured = self.mcp_servers.len();
        let enabled = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Enabled)
            .count();
        let disabled = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Disabled)
            .count();
        let failed = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Failed)
            .count();
        (configured, enabled, disabled, failed)
    }

    pub(crate) fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        self.set_llm_visible_mcp_tool_count(llm_visible_mcp_tool_count);
        self.set_mcp_server_state_by_name_with_reason(server_name, state, reason);
    }
}

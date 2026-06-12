use super::*;

impl AppState {
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
        if self.mcp_servers.is_empty() {
            self.mcp_panel_selection = 0;
        } else if self.mcp_panel_selection >= self.mcp_servers.len() {
            self.mcp_panel_selection = self.mcp_servers.len().saturating_sub(1);
        }
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
        let len = self.mcp_servers.len();
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
        let len = self.mcp_servers.len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = (self.mcp_panel_selection + 1) % len;
    }

    pub fn selected_mcp_server_name(&self) -> Option<&str> {
        self.mcp_servers
            .get(self.mcp_panel_selection)
            .map(|server| server.name.as_str())
    }

    pub fn selected_mcp_server_state(&self) -> Option<McpServerUsabilityState> {
        self.mcp_servers
            .get(self.mcp_panel_selection)
            .map(|server| server.state)
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
        let Some(server) = self.mcp_servers.get_mut(self.mcp_panel_selection) else {
            return false;
        };

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
}

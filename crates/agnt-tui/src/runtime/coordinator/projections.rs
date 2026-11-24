//! The surface's projection setters: the state the loop, the pickers, and the
//! command layer hand in, written straight onto the surface's state.

use super::RuntimeCoordinator;
use crate::state::{McpServerState, McpServerUsabilityState};
use agnt_runtime::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use agnt_runtime::tools::mcp::runtime::McpServerLifecycle;

impl RuntimeCoordinator {
    /// Fold the runtime's server lifecycle: the row each server takes, its
    /// visible tool count, and — for a server that failed — the reason it gave.
    /// The reasons are written after the rows so a fresh projection prunes the
    /// ones it no longer reports.
    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        let entries = projection
            .into_iter()
            .map(|server| {
                self.state
                    .status
                    .mcp
                    .set_mcp_visible_tool_count_by_server_name(
                        server.name.as_str(),
                        server.visible_tool_count,
                    );
                (
                    server.name,
                    match (server.enabled, server.connected) {
                        (true, true) => McpServerUsabilityState::Enabled,
                        (true, false) => McpServerUsabilityState::Failed,
                        (false, _) => McpServerUsabilityState::Disabled,
                    },
                    server.failure_reason,
                )
            })
            .collect::<Vec<_>>();
        self.state.status.mcp.set_mcp_servers(
            entries
                .iter()
                .map(|(name, state, _)| McpServerState {
                    name: name.clone(),
                    state: *state,
                })
                .collect(),
        );
        for (name, state, reason) in entries {
            if reason.is_some() {
                self.state
                    .status
                    .mcp
                    .set_mcp_server_state_by_name_with_reason(&name, state, reason);
            }
        }
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        let mapped = skills
            .into_iter()
            .map(|skill| crate::state::DiscoverableSkill {
                source_priority: skill.source.priority(),
                source: skill.source.label().to_string(),
                name: skill.name,
                description: skill.description,
            })
            .collect();
        self.state.status.mcp.set_discoverable_skills(mapped);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.state.status.mcp.mark_skills_discovery_failed();
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.state.status.mcp.set_llm_visible_mcp_tool_count(count);
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.state
            .status
            .mcp
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .status
            .mcp
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state
            .status
            .tokens
            .set_context_window_max_tokens(max_tokens);
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.state.set_active_agent_identity(name);
    }

    pub fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.state.status.identity.active_persona_icon = icon;
    }

    pub fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.state.status.identity.agent_cycle_names = names;
    }
}

use crate::protocol::contracts::McpUsabilityState;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::{
    config::McpServerConfig,
    runtime::{McpRuntime, McpServerLifecycle},
};
use crate::types::ToolDefinition;

use super::super::mcp_helpers::{
    rebuild_mcp_lifecycle_projection, stage_enabled_mcp_runtime_state,
};

pub struct McpState {
    mcp_runtime: Option<McpRuntime>,
    mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    mcp_server_configs: Vec<McpServerConfig>,
    mcp_caller_cwd: Option<std::path::PathBuf>,
    mcp_registry: McpToolRegistry,
    max_tool_result_bytes: usize,
}

impl McpState {
    pub fn new(
        mcp_runtime: Option<McpRuntime>,
        mcp_lifecycle_projection: Vec<McpServerLifecycle>,
        mcp_server_configs: Vec<McpServerConfig>,
        mcp_caller_cwd: Option<std::path::PathBuf>,
        mcp_registry: McpToolRegistry,
        max_tool_result_bytes: usize,
    ) -> Self {
        Self {
            mcp_runtime,
            mcp_lifecycle_projection,
            mcp_server_configs,
            mcp_caller_cwd,
            mcp_registry,
            max_tool_result_bytes,
        }
    }

    pub fn mcp_registry(&self) -> &McpToolRegistry {
        &self.mcp_registry
    }
    pub fn mcp_caller_cwd(&self) -> Option<&std::path::Path> {
        self.mcp_caller_cwd.as_deref()
    }
    pub fn mcp_lifecycle_projection(&self) -> &[McpServerLifecycle] {
        &self.mcp_lifecycle_projection
    }

    pub async fn set_mcp_server_enabled(
        &mut self,
        tool_server_handle: &rig::tool::server::ToolServerHandle,
        server_name: &str,
        enabled: bool,
        tool_definitions: &mut Vec<ToolDefinition>,
    ) -> Result<McpUsabilityState, String> {
        if !enabled {
            log::info!("MCP disable: server={server_name}");
            // Disable path: just toggle visibility in the registry.
            // Sessions stay alive and tools remain registered on the handle —
            // McpToolRegistry.contains() gates LLM visibility, so tools are hidden
            // without any disconnection or remove_tool calls.
            self.mcp_registry.set_server_enabled(server_name, false)?;
            // Mark disconnected so lifecycle projection shows connected: false
            if let Some(rt) = self.mcp_runtime.as_mut() {
                rt.mark_disconnected(server_name);
            }
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                tool_definitions,
            );
            return Ok(McpUsabilityState::Disabled);
        }

        if !self
            .mcp_server_configs
            .iter()
            .any(|server| server.name == server_name)
        {
            log::debug!("MCP enable: server={server_name} not in config, marking failed");
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                tool_definitions,
            );
            return Ok(McpUsabilityState::Failed);
        }

        // Enable path: check if a session already exists for this server.
        let already_connected = self
            .mcp_runtime
            .as_ref()
            .is_some_and(|rt| rt.has_server(server_name));

        log::debug!("MCP enable: server={server_name} already_connected={already_connected}");

        if already_connected {
            log::info!("MCP enable Case A (re-enable visibility): server={server_name}");
            // Case A: Session is alive, tools are registered on the handle.
            // Just re-enable visibility in the registry — no reconnection needed.
            self.mcp_registry.set_server_enabled(server_name, true)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                tool_definitions,
            );
            return Ok(McpUsabilityState::Enabled);
        }

        // Case B: Server has never been connected (configured with enabled: false at
        // startup, or first-time enable). Connect only this single server.
        log::info!("MCP enable Case B (first-time connect): server={server_name}");
        // Force enabled: true so select_enabled_servers() doesn't filter it out.
        let single_server_config: Vec<McpServerConfig> = self
            .mcp_server_configs
            .iter()
            .filter(|s| s.name == server_name)
            .map(|s| McpServerConfig {
                enabled: true,
                ..s.clone()
            })
            .collect();

        match crate::tools::mcp::runtime::connect_servers(
            tool_server_handle,
            &single_server_config,
            self.mcp_caller_cwd.as_deref(),
            self.max_tool_result_bytes,
        )
        .await
        {
            Ok(new_rt) if new_rt.has_sessions() => {
                let discovered = new_rt.discovered_tools().to_vec();
                log::info!(
                    "MCP connect succeeded: server={server_name} discovered={}",
                    discovered.len()
                );

                let (staged_tool_definitions, staged_registry) = stage_enabled_mcp_runtime_state(
                    tool_definitions,
                    &self.mcp_registry,
                    server_name,
                    &discovered,
                )?;

                *tool_definitions = staged_tool_definitions;
                self.mcp_registry = staged_registry;

                // Merge the new sessions into the existing runtime (or set it if None).
                match self.mcp_runtime.as_mut() {
                    Some(existing) => existing.merge(new_rt),
                    None => self.mcp_runtime = Some(new_rt),
                }

                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    tool_definitions,
                );

                Ok(McpUsabilityState::Enabled)
            }
            Ok(_) => {
                log::warn!("MCP connect returned no sessions: server={server_name}");
                self.mcp_registry.set_server_enabled(server_name, false)?;
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    tool_definitions,
                );
                Ok(McpUsabilityState::Failed)
            }
            Err(e) => {
                log::warn!("MCP connect failed: server={server_name} error={e}");
                self.mcp_registry.set_server_enabled(server_name, false)?;
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    tool_definitions,
                );
                Ok(McpUsabilityState::Failed)
            }
        }
    }

    pub fn llm_visible_mcp_tool_count(&self, active_tool_definitions: &[ToolDefinition]) -> usize {
        active_tool_definitions
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
            .count()
    }

    pub fn llm_visible_mcp_tool_count_for_server(
        &self,
        server_name: &str,
        active_tool_definitions: &[ToolDefinition],
    ) -> usize {
        active_tool_definitions
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
            .filter_map(|tool| self.mcp_registry.server_name_for(tool.name.as_str()))
            .filter(|server| *server == server_name)
            .count()
    }

    pub fn llm_visible_mcp_tool_names_by_server(
        &self,
        active_tool_definitions: &[ToolDefinition],
    ) -> Vec<(String, Vec<String>)> {
        let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();

        for tool in active_tool_definitions
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
        {
            let Some(server_name) = self.mcp_registry.server_name_for(tool.name.as_str()) else {
                continue;
            };
            grouped
                .entry(server_name.to_string())
                .or_default()
                .push(tool.name.clone());
        }

        grouped
            .into_iter()
            .map(|(server, mut names)| {
                names.sort();
                names.dedup();
                (server, names)
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "mcp_test.rs"]
mod mcp_test;

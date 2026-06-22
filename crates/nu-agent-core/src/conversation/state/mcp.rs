use crate::protocol::contracts::McpUsabilityState;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::{
    config::McpServerConfig,
    runtime::{McpRuntime, McpServerLifecycle},
};
use crate::types::ToolDefinition;

use super::super::mcp_helpers::{
    mcp_enable_runtime_config, rebuild_mcp_lifecycle_projection, stage_enabled_mcp_runtime_state,
};

pub struct McpState {
    mcp_runtime: Option<McpRuntime>,
    mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    mcp_server_configs: Vec<McpServerConfig>,
    mcp_caller_cwd: Option<std::path::PathBuf>,
    mcp_registry: McpToolRegistry,
}

impl McpState {
    pub fn new(
        mcp_runtime: Option<McpRuntime>,
        mcp_lifecycle_projection: Vec<McpServerLifecycle>,
        mcp_server_configs: Vec<McpServerConfig>,
        mcp_caller_cwd: Option<std::path::PathBuf>,
        mcp_registry: McpToolRegistry,
    ) -> Self {
        Self {
            mcp_runtime,
            mcp_lifecycle_projection,
            mcp_server_configs,
            mcp_caller_cwd,
            mcp_registry,
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

    pub fn set_mcp_server_enabled(
        &mut self,
        tool_server_handle: &rig::tool::server::ToolServerHandle,
        server_name: &str,
        enabled: bool,
        tool_definitions: &mut Vec<ToolDefinition>,
        runtime: &tokio::runtime::Handle,
    ) -> Result<McpUsabilityState, String> {
        if !enabled {
            // Remove this server's tools from the handle so they are no longer executable.
            // This also prevents duplicate add_tool errors on re-enable.
            let tool_names_to_remove: Vec<String> = tool_definitions
                .iter()
                .filter(|t| {
                    self.mcp_registry
                        .server_name_for(t.name.as_str())
                        == Some(server_name)
                })
                .map(|t| t.name.clone())
                .collect();

            for name in &tool_names_to_remove {
                if let Err(e) =
                    runtime.block_on(async { tool_server_handle.remove_tool(name).await })
                {
                    log::warn!("Failed to remove tool '{name}' on MCP server disable: {e}");
                }
            }

            self.mcp_registry.set_server_enabled(server_name, false)?;
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
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                tool_definitions,
            );
            return Ok(McpUsabilityState::Failed);
        }

        let runtime_config =
            mcp_enable_runtime_config(&self.mcp_server_configs, &self.mcp_registry, server_name);

        match runtime.block_on(crate::tools::mcp::runtime::connect_servers(
            tool_server_handle,
            &runtime_config,
            self.mcp_caller_cwd.as_deref(),
        )) {
            Ok(rt) if rt.has_sessions() => {
                let discovered = rt.discovered_tools().to_vec();

                let (staged_tool_definitions, staged_registry) = stage_enabled_mcp_runtime_state(
                    tool_definitions,
                    &self.mcp_registry,
                    server_name,
                    &discovered,
                )?;

                *tool_definitions = staged_tool_definitions;
                self.mcp_registry = staged_registry;
                self.mcp_runtime = Some(rt);
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    tool_definitions,
                );

                Ok(McpUsabilityState::Enabled)
            }
            Ok(_) | Err(_) => {
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

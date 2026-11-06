use super::super::managers::ToolManager;
use crate::types::ToolDefinition;
use agnt_runtime::tools::authz::PermissionsConfig;
use agnt_runtime::tools::closure::ClosureRegistry;
use agnt_runtime::tools::handler::{self, McpToolRegistry};

pub struct ToolState {
    tool_definitions: Vec<ToolDefinition>,
    baseline_tool_definitions: Vec<ToolDefinition>,
    closure_registry: ClosureRegistry,
}

impl ToolState {
    pub fn new(
        tool_definitions: Vec<ToolDefinition>,
        baseline_tool_definitions: Vec<ToolDefinition>,
        closure_registry: ClosureRegistry,
    ) -> Self {
        Self {
            tool_definitions,
            baseline_tool_definitions,
            closure_registry,
        }
    }

    pub fn closure_registry(&self) -> &ClosureRegistry {
        &self.closure_registry
    }

    pub fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_definitions
    }

    pub fn tool_definitions_mut(&mut self) -> &mut Vec<ToolDefinition> {
        &mut self.tool_definitions
    }

    pub fn reset_to_baseline(&mut self) {
        self.tool_definitions = self.baseline_tool_definitions.clone();
    }

    pub fn active_definitions(
        &self,
        mcp_registry: &McpToolRegistry,
        permissions: &PermissionsConfig,
    ) -> Vec<ToolDefinition> {
        handler::llm_visible_tool_definitions(&self.tool_definitions, mcp_registry, permissions)
    }
}

impl ToolManager for ToolState {
    fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_definitions
    }

    fn closure_registry(&self) -> &ClosureRegistry {
        &self.closure_registry
    }

    fn tool_definitions_mut(&mut self) -> &mut Vec<ToolDefinition> {
        &mut self.tool_definitions
    }

    fn reset_to_baseline(&mut self) {
        self.reset_to_baseline()
    }

    fn active_definitions(
        &self,
        mcp_registry: &McpToolRegistry,
        permissions: &PermissionsConfig,
    ) -> Vec<ToolDefinition> {
        self.active_definitions(mcp_registry, permissions)
    }
}

#[cfg(test)]
#[path = "../../../test/tools/mcp/namespaced.rs"]
mod namespaced_mcp_test;

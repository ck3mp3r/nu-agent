use crate::tools::authz::PermissionsConfig;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::{self, McpToolRegistry};
use crate::types::ToolDefinition;

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

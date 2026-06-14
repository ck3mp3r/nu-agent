use crate::agent::tools::authz::PermissionsConfig;
use crate::agent::tools::handler::{self, McpToolRegistry};
use crate::tools::closure::ClosureRegistry;
use crate::types::ToolDefinition;

pub(crate) struct ToolState {
    tool_definitions: Vec<ToolDefinition>,
    baseline_tool_definitions: Vec<ToolDefinition>,
    closure_registry: ClosureRegistry,
}

impl ToolState {
    pub(crate) fn new(
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

    pub(crate) fn closure_registry(&self) -> &ClosureRegistry {
        &self.closure_registry
    }

    pub(crate) fn tool_definitions_mut(&mut self) -> &mut Vec<ToolDefinition> {
        &mut self.tool_definitions
    }

    pub(crate) fn reset_to_baseline(&mut self) {
        self.tool_definitions = self.baseline_tool_definitions.clone();
    }

    pub(crate) fn active_definitions(
        &self,
        mcp_registry: &McpToolRegistry,
        permissions: &PermissionsConfig,
    ) -> Vec<ToolDefinition> {
        handler::llm_visible_tool_definitions(&self.tool_definitions, mcp_registry, permissions)
    }
}

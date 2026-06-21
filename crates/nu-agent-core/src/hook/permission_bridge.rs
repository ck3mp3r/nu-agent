//! Bridges the PermissionResolver trait to the existing authorization system.

use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::{
    McpToolRegistry, ToolSource, is_builtin_fs_tool_name, is_builtin_tool_name,
};

/// Resolve the source of a tool by checking the closure and MCP registries.
pub fn resolve_tool_source(
    name: &str,
    closures: &ClosureRegistry,
    mcp: &McpToolRegistry,
) -> ToolSource {
    if closures.get(name).is_some() {
        ToolSource::Closure
    } else if is_builtin_fs_tool_name(name) {
        ToolSource::BuiltinFs
    } else if is_builtin_tool_name(name) {
        ToolSource::Builtin
    } else if mcp.contains(name) {
        ToolSource::Mcp
    } else {
        ToolSource::Unknown
    }
}

#[cfg(test)]
#[path = "permission_bridge_test.rs"]
mod permission_bridge_test;

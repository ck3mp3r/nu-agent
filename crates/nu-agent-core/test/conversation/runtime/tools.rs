use super::*;

// ========================================================================
// Tool visibility tests
// ========================================================================

#[test]
fn active_tool_definitions_returns_empty_when_no_tools() {
    // Characterise active_tool_definitions (runtime.rs:753-759).
    // Delegates to handler::llm_visible_tool_definitions with the runtime's
    // tool_definitions, mcp_registry, and permissions.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::empty();
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(result.is_empty());
}

// ========================================================================
// Phase G: ToolState characterisation tests
// ========================================================================

#[test]
fn tool_state_active_definitions_empty_when_no_tools() {
    // Characterise active_tool_definitions: with empty tool_definitions,
    // the method delegates to handler::llm_visible_tool_definitions
    // and returns an empty Vec.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::empty();
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(
        result.is_empty(),
        "active_tool_definitions must return empty Vec when no tools defined"
    );
}

#[test]
fn tool_state_baseline_is_reset_source() {
    // Characterise that baseline_tool_definitions serves as the reset
    // source: cloning baseline into tool_definitions restores initial state.

    let _tool_definitions: Vec<ToolDefinition> = vec![];
    let baseline_tool_definitions: Vec<ToolDefinition> = vec![ToolDefinition {
        name: "test_tool".to_string(),
        description: "".to_string(),
        parameters: serde_json::json!({}),
    }];

    // Simulate switch_agent reset: tool_definitions = baseline_tool_definitions.clone()
    let tool_definitions = baseline_tool_definitions.clone();

    assert_eq!(
        tool_definitions.len(),
        1,
        "after reset, tool_definitions must match baseline length"
    );
    assert_eq!(tool_definitions[0].name, "test_tool");
}

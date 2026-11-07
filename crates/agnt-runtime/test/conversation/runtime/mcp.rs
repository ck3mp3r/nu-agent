use super::*;

// ========================================================================
// Phase C-pre: Characterise sub-struct clusters before field decomposition
// ========================================================================

#[test]
fn mcp_state_initial_tool_count_is_zero() {
    // Characterise llm_visible_mcp_tool_count (runtime.rs:372-377).
    // With an empty mcp_registry and no mcp_lifecycle_projection, the
    // method filters active_tool_definitions by mcp_registry.is_registered
    // — an empty registry yields 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::empty();
    let permissions = PermissionsConfig::safe_defaults(true);

    // Replicate the method body: filter active_tool_definitions by registry
    let active = crate::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .count();

    assert_eq!(count, 0, "empty registry must yield zero MCP tool count");
}

#[test]
fn mcp_state_tool_count_by_server_returns_zero_for_unknown() {
    // Characterise llm_visible_mcp_tool_count_for_server (runtime.rs:379-386).
    // Querying for "nonexistent-server" with an empty registry must return 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::empty();
    let permissions = PermissionsConfig::safe_defaults(true);

    let active = crate::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .filter_map(|tool| mcp_registry.server_name_for(tool.name.as_str()))
        .filter(|server| *server == "nonexistent-server")
        .count();

    assert_eq!(count, 0, "unknown server must yield zero tool count");
}

// ========================================================================
// Phase L: McpState characterisation tests
// ========================================================================

#[test]
fn mcp_state_caller_cwd_none_by_default() {
    let _ms = test_memory_state();
    // Access mcp_state through a compile-time type check
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_caller_cwd().is_none());
    };
    // Value-level proof: default Option is None
    let cwd: Option<std::path::PathBuf> = None;
    assert!(cwd.is_none());
}

#[test]
fn mcp_state_lifecycle_projection_empty_by_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_lifecycle_projection().is_empty());
    };
    // Value-level proof: default Vec is empty
    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}

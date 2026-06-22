use super::*;

use crate::conversation::test_helpers::{mcp_server_config, mcp_tool, tool_definition_named};
use crate::tools::handler::McpToolRegistry;

/// Helper: build a minimal McpState with one server ("k8s") and two tools registered.
fn mcp_state_with_k8s_tools() -> (McpState, Vec<ToolDefinition>) {
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("k8s", "k8s__delete_pod", "delete_pod"),
    ])
    .expect("registry");

    let configs = vec![mcp_server_config("k8s", true)];

    let tool_definitions = vec![
        tool_definition_named("read"),
        tool_definition_named("k8s__list_pods"),
        tool_definition_named("k8s__delete_pod"),
    ];

    let state = McpState::new(
        None,
        vec![],
        configs,
        None,
        registry,
    );

    (state, tool_definitions)
}

#[test]
fn disable_returns_disabled_state() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rig::tool::server::ToolServer::new().run();

    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle())
        .expect("set_mcp_server_enabled should not error");

    assert_eq!(result, McpUsabilityState::Disabled);
}

#[test]
fn disable_marks_server_as_not_enabled_in_registry() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle())
        .expect("set_mcp_server_enabled should not error");

    // Registry must no longer report the server as enabled.
    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
}

#[test]
fn disable_makes_tools_invisible_via_registry_contains() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rig::tool::server::ToolServer::new().run();

    // Pre-condition: both tools are visible before disable.
    assert!(mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("k8s__delete_pod"));

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle())
        .expect("set_mcp_server_enabled should not error");

    // After disable, tools should not be accessible via `contains` (enabled check fails).
    // NOTE: We verify registry state here because registering a real ToolDyn on
    // ToolServerHandle requires a full rig `Tool` impl, which is out of scope for
    // a unit test of the disable path. The remove_tool call itself is exercised and
    // any errors are gracefully swallowed (remove on empty handle returns an error
    // that the code logs and continues — verified by absence of panic).
    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(!mcp_state.mcp_registry().contains("k8s__delete_pod"));
}

#[test]
fn disable_leaves_non_target_server_tools_enabled() {
    // Setup two servers; only "k8s" is disabled.
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("gh", "gh__list_prs", "list_prs"),
    ])
    .expect("registry");

    let configs = vec![
        mcp_server_config("k8s", true),
        mcp_server_config("gh", true),
    ];

    let mut tool_definitions = vec![
        tool_definition_named("read"),
        tool_definition_named("k8s__list_pods"),
        tool_definition_named("gh__list_prs"),
    ];

    let mut mcp_state = McpState::new(None, vec![], configs, None, registry);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle())
        .expect("set_mcp_server_enabled should not error");

    // "k8s" tools disabled; "gh" tools untouched.
    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("gh__list_prs"));
    assert!(mcp_state.mcp_registry().is_server_enabled("gh"));
}

#[test]
fn disable_remove_tool_errors_are_silently_swallowed() {
    // Calling remove_tool on a handle for a tool that was never added returns an
    // error. The fix must NOT panic — errors are logged and ignored.
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    // Fresh handle: k8s tools were never added_tool'd here.
    let handle = rig::tool::server::ToolServer::new().run();

    // Must not panic even though remove_tool will return errors (tool not found).
    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle());

    assert!(result.is_ok(), "disable must succeed even when remove_tool fails: {result:?}");
}

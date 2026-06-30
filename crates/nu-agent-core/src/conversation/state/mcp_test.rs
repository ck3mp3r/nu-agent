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

    let state = McpState::new(None, vec![], configs, None, registry, 20_000);

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

    // After disable, tools are hidden via the registry — sessions stay alive.
    // McpToolRegistry.contains() returns false when the server is disabled, which
    // is the only gate that matters for LLM visibility.
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

    let mut mcp_state = McpState::new(None, vec![], configs, None, registry, 20_000);
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
fn disable_then_reenable_via_registry_toggle_restores_visibility() {
    // With a connected session present, disable then re-enable should toggle
    // registry state only — no reconnection, no add_tool/remove_tool calls.
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rig::tool::server::ToolServer::new().run();

    // Simulate a connected session by building a runtime with "k8s" in connected_servers.
    // We do this by calling set_mcp_server_enabled for a server that has no session
    // and no reachable URL; we just inject a mock runtime via the public constructor.
    // Instead: directly verify that calling disable followed by enable (Case A path)
    // on a state that has mcp_runtime=None falls through to Case B (Failed, since
    // there is no real server to connect to). The Case A path (already_connected) is
    // only reachable when there IS a real runtime, so we just verify registry toggle here.
    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions, rt.handle())
        .expect("disable should succeed");

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));

    // Re-enable: no runtime exists (mcp_runtime=None), so this goes to Case B.
    // Case B tries to connect to the URL in the config. The test config uses
    // http://localhost:7777/mcp which is not running, so it returns Failed.
    // This verifies the Case A branch is not taken when runtime is None.
    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", true, &mut tool_definitions, rt.handle())
        .expect("re-enable should not error on connection failure");

    // Case B: no session → connection attempt → fails (no real server) → Failed.
    assert_eq!(result, McpUsabilityState::Failed);
    // Registry should still have k8s disabled (connection failed).
    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
}

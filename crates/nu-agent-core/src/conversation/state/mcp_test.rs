use super::*;

use crate::conversation::test_helpers::{mcp_server_config, mcp_tool, tool_definition_named};
use crate::tools::handler::McpToolRegistry;

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

#[tokio::test]
async fn disable_returns_disabled_state() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let handle = rig::tool::server::ToolServer::new().run();

    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .expect("set_mcp_server_enabled should not error");

    assert_eq!(result, McpUsabilityState::Disabled);
}

#[tokio::test]
async fn disable_marks_server_as_not_enabled_in_registry() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .expect("set_mcp_server_enabled should not error");

    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
}

#[tokio::test]
async fn disable_makes_tools_invisible_via_registry_contains() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let handle = rig::tool::server::ToolServer::new().run();

    assert!(mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("k8s__delete_pod"));

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .expect("set_mcp_server_enabled should not error");

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(!mcp_state.mcp_registry().contains("k8s__delete_pod"));
}

#[tokio::test]
async fn disable_leaves_non_target_server_tools_enabled() {
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
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .expect("set_mcp_server_enabled should not error");

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("gh__list_prs"));
    assert!(mcp_state.mcp_registry().is_server_enabled("gh"));
}

#[tokio::test]
async fn disable_then_reenable_via_registry_toggle_restores_visibility() {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools();
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .expect("disable should succeed");

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));

    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", true, &mut tool_definitions)
        .await
        .expect("re-enable should not error on connection failure");

    assert_eq!(result, McpUsabilityState::Failed);
    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
}

use super::*;

use crate::conversation::test_helpers::{mcp_server_config, mcp_tool, tool_definition_named};
use crate::tools::handler::McpToolRegistry;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn mcp_state_with_k8s_tools() -> Result<(McpState, Vec<ToolDefinition>)> {
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("k8s", "k8s__delete_pod", "delete_pod"),
    ])
    .map_err(|e| format!("registry: {e:?}"))?;

    let configs = vec![mcp_server_config("k8s", true)];

    let tool_definitions = vec![
        tool_definition_named("read")?,
        tool_definition_named("k8s__list_pods")?,
        tool_definition_named("k8s__delete_pod")?,
    ];

    let state = McpState::new(None, vec![], configs, None, registry, 20_000, None);

    Ok((state, tool_definitions))
}

#[tokio::test]
async fn disable_returns_disabled_state() -> Result<()> {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools()?;
    let handle = rig::tool::server::ToolServer::new().run();

    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("set_mcp_server_enabled should not error: {e:?}"))?;

    assert_eq!(result, McpUsabilityState::Disabled);
    Ok(())
}

#[tokio::test]
async fn disable_marks_server_as_not_enabled_in_registry() -> Result<()> {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools()?;
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("set_mcp_server_enabled should not error: {e:?}"))?;

    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
    Ok(())
}

#[tokio::test]
async fn disable_makes_tools_invisible_via_registry_contains() -> Result<()> {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools()?;
    let handle = rig::tool::server::ToolServer::new().run();

    assert!(mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("k8s__delete_pod"));

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("set_mcp_server_enabled should not error: {e:?}"))?;

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(!mcp_state.mcp_registry().contains("k8s__delete_pod"));
    Ok(())
}

#[tokio::test]
async fn disable_leaves_non_target_server_tools_enabled() -> Result<()> {
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("gh", "gh__list_prs", "list_prs"),
    ])
    .map_err(|e| format!("registry: {e:?}"))?;

    let configs = vec![
        mcp_server_config("k8s", true),
        mcp_server_config("gh", true),
    ];

    let mut tool_definitions = vec![
        tool_definition_named("read")?,
        tool_definition_named("k8s__list_pods")?,
        tool_definition_named("gh__list_prs")?,
    ];

    let mut mcp_state = McpState::new(None, vec![], configs, None, registry, 20_000, None);
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("set_mcp_server_enabled should not error: {e:?}"))?;

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));
    assert!(mcp_state.mcp_registry().contains("gh__list_prs"));
    assert!(mcp_state.mcp_registry().is_server_enabled("gh"));
    Ok(())
}

#[tokio::test]
async fn disable_then_reenable_via_registry_toggle_restores_visibility() -> Result<()> {
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_k8s_tools()?;
    let handle = rig::tool::server::ToolServer::new().run();

    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("disable should succeed: {e:?}"))?;

    assert!(!mcp_state.mcp_registry().contains("k8s__list_pods"));

    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", true, &mut tool_definitions)
        .await
        .map_err(|e| format!("re-enable should not error on connection failure: {e:?}"))?;

    assert_eq!(result, McpUsabilityState::Failed);
    assert!(!mcp_state.mcp_registry().is_server_enabled("k8s"));
    Ok(())
}

/// A server whose connect always fails: the stdio transport with no command.
fn mcp_state_with_unconnectable_server() -> Result<(McpState, Vec<ToolDefinition>)> {
    let registry =
        McpToolRegistry::from_tools(vec![mcp_tool("k8s", "k8s__list_pods", "list_pods")])
            .map_err(|e| format!("registry: {e:?}"))?;

    let mut config = mcp_server_config("k8s", true);
    config.transport = crate::tools::mcp::config::McpTransportType::Stdio;
    config.command = None;

    let tool_definitions = vec![tool_definition_named("read")?];
    let state = McpState::new(None, vec![], vec![config], None, registry, 20_000, None);

    Ok((state, tool_definitions))
}

/// WHEN the toggle's connect fails, THE runtime state SHALL carry the reason and
/// the lifecycle projection SHALL show the server down: the panel's error row
/// reads the reason instead of `Error: None`.
#[tokio::test]
async fn failed_toggle_records_the_reason() -> Result<()> {
    // -- Setup & Fixtures
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_unconnectable_server()?;
    let handle = rig::tool::server::ToolServer::new().run();

    // -- Exec
    let result = mcp_state
        .set_mcp_server_enabled(&handle, "k8s", true, &mut tool_definitions)
        .await
        .map_err(|e| format!("a failed connect must not error the toggle: {e:?}"))?;

    // -- Check: the state carries the reason, one control-free row.
    assert_eq!(result, McpUsabilityState::Failed);
    let reason = mcp_state
        .mcp_failure_reason("k8s")
        .ok_or("the failed connect must carry its reason")?;
    assert!(
        reason.contains("requires command"),
        "the carried reason must name the cause, got: {reason}"
    );
    assert!(!reason.chars().any(char::is_control));

    // -- Check: the projection shows the server down after the failed toggle.
    let lifecycle = mcp_state
        .mcp_lifecycle_projection()
        .iter()
        .find(|server| server.name == "k8s")
        .ok_or("the projection must list the toggled server")?;
    assert!(!lifecycle.connected);
    Ok(())
}

/// WHEN a failed server is disabled, THE recorded reason SHALL NOT outlive the
/// failure it describes.
#[tokio::test]
async fn a_failure_reason_does_not_outlive_the_failure() -> Result<()> {
    // -- Setup & Fixtures
    let (mut mcp_state, mut tool_definitions) = mcp_state_with_unconnectable_server()?;
    let handle = rig::tool::server::ToolServer::new().run();
    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", true, &mut tool_definitions)
        .await
        .map_err(|e| format!("a failed connect must not error the toggle: {e:?}"))?;

    // -- Exec: disabling the server drops the failure it no longer describes.
    mcp_state
        .set_mcp_server_enabled(&handle, "k8s", false, &mut tool_definitions)
        .await
        .map_err(|e| format!("disable should not error: {e:?}"))?;

    // -- Check
    assert_eq!(mcp_state.mcp_failure_reason("k8s"), None);
    Ok(())
}

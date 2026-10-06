use super::*;

use crate::conversation::test_helpers::{mcp_server_config, mcp_tool, tool_definition_named};
use crate::tools::handler::McpToolRegistry;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn enabling_startup_disabled_server_registers_all_discovered_mcp_tools() -> Result<()> {
    let mut tool_definitions = vec![tool_definition_named("read")];
    let mut registry =
        McpToolRegistry::from_tools(vec![mcp_tool("gh", "gh__list_prs", "list_prs")])
            .map_err(|e| format!("startup registry: {e:?}"))?;

    let discovered_from_toggle = vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("k8s", "k8s__delete_pod", "delete_pod"),
    ];

    merge_new_mcp_tools_into_runtime(
        &mut tool_definitions,
        &mut registry,
        &discovered_from_toggle,
    )
    .map_err(|e| format!("toggle merge should succeed: {e:?}"))?;

    let permissions = crate::tools::authz::PermissionsConfig::safe_defaults(true);
    let visible = crate::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &registry,
        &permissions,
    );

    assert!(visible.iter().any(|tool| tool.name == "k8s__list_pods"));
    assert!(visible.iter().any(|tool| tool.name == "k8s__delete_pod"));
    assert_eq!(
        visible
            .iter()
            .filter(|tool| tool.name.starts_with("k8s__"))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn enabling_startup_disabled_server_registers_dispatch_raw_name_mapping() -> Result<()> {
    let mut tool_definitions = vec![tool_definition_named("read")];
    let mut registry = McpToolRegistry::empty();

    let discovered = vec![mcp_tool("k8s", "k8s__list_pods", "list_pods")];

    merge_new_mcp_tools_into_runtime(&mut tool_definitions, &mut registry, &discovered)
        .map_err(|e| format!("toggle merge should succeed: {e:?}"))?;

    assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
    assert!(registry.contains("k8s__list_pods"));
    Ok(())
}

#[test]
fn enabling_stage_conflict_is_transactional_and_keeps_original_runtime_state() -> Result<()> {
    let tool_definitions = vec![tool_definition_named("read")];
    let registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "k8s__list_pods", "list_pods")])
        .map_err(|e| format!("startup registry: {e:?}"))?;

    let discovered_conflict = vec![mcp_tool("k8s", "k8s__list_pods", "list_all_pods")];

    let result =
        stage_enabled_mcp_runtime_state(&tool_definitions, &registry, "k8s", &discovered_conflict);

    assert!(result.is_err());
    let err = result
        .err()
        .ok_or("should fail on conflicting raw mapping")?;
    assert!(
        err.contains("conflicting raw MCP tool mapping"),
        "must mention conflicting raw mapping, got: {err}"
    );

    assert_eq!(
        tool_definitions.len(),
        1,
        "tool definitions must remain unchanged"
    );
    assert_eq!(tool_definitions[0].name, "read");

    assert!(
        registry.contains("k8s__list_pods"),
        "existing registry mapping must remain visible"
    );
    assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
    assert!(
        !registry.is_server_enabled("k8s"),
        "new server must not be enabled on conflict"
    );
    Ok(())
}

#[test]
fn lifecycle_projection_recomputes_from_registry_state_without_runtime() -> Result<()> {
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("gh", "gh__list_prs", "list_prs"),
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
    ])
    .map_err(|e| format!("registry: {e:?}"))?;
    registry
        .set_server_enabled("k8s", false)
        .map_err(|e| format!("disable k8s: {e:?}"))?;

    let configs = vec![
        mcp_server_config("k8s", true),
        mcp_server_config("gh", true),
    ];

    let projection = rebuild_mcp_lifecycle_projection(
        None,
        &configs,
        &registry,
        &[
            tool_definition_named("read"),
            tool_definition_named("gh__list_prs"),
            tool_definition_named("k8s__list_pods"),
        ],
    );

    assert_eq!(projection.len(), 2);
    assert_eq!(projection[0].name, "gh");
    assert!(projection[0].enabled);
    assert!(!projection[0].connected);
    assert_eq!(projection[0].visible_tool_count, 1);

    assert_eq!(projection[1].name, "k8s");
    assert!(!projection[1].enabled);
    assert!(!projection[1].connected);
    assert_eq!(projection[1].visible_tool_count, 0);
    Ok(())
}

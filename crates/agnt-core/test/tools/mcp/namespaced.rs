use crate::conversation::state::ToolState;
use crate::types::{EmptyToolName, ToolDefinition, ToolName};
use agnt_runtime::tools::authz::PermissionsConfig;
use agnt_runtime::tools::closure::ClosureRegistry;
use agnt_runtime::tools::handler::McpToolRegistry;
use agnt_runtime::tools::mcp::client::McpToolDefinition;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// McpToolRegistry visibility through ToolState::active_definitions
// ---------------------------------------------------------------------------

/// A `ToolDefinition` with a minimal schema.
fn tool_def(name: &str) -> core::result::Result<ToolDefinition, EmptyToolName> {
    Ok(ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: ToolName::new(name)?,
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type": "object"}),
    })
}

/// An `McpToolDefinition` exposing `raw` from `server` as `server__raw`.
fn mcp_tool(server: &str, raw: &str) -> McpToolDefinition {
    McpToolDefinition {
        server: server.to_string(),
        name: format!("{server}__{raw}"),
        raw_name: raw.to_string(),
        description: Some(format!("{raw} on {server}")),
        parameters: None,
    }
}

/// A `ToolState` whose definition set is exactly `names`.
fn tool_state(names: &[&str]) -> core::result::Result<ToolState, EmptyToolName> {
    let defs: Vec<ToolDefinition> = names
        .iter()
        .map(|name| tool_def(name))
        .collect::<core::result::Result<_, _>>()?;
    Ok(ToolState::new(
        defs.clone(),
        defs,
        ClosureRegistry::default(),
    ))
}

/// The names `active_definitions` exposes for `state` and `registry`.
fn visible_names(state: &ToolState, registry: &McpToolRegistry) -> Vec<String> {
    state
        .active_definitions(registry, &PermissionsConfig::safe_defaults(true))
        .into_iter()
        .map(|def| def.name.as_str().to_string())
        .collect()
}

#[test]
fn mcp_registry_disabled_server_tools_are_hidden_from_active_definitions() -> Result<()> {
    // -- Setup & Fixtures
    let state = tool_state(&["read", "gh__list_prs", "k8s__list_pods"])?;
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("gh", "list_prs"),
        mcp_tool("k8s", "list_pods"),
    ])
    .map_err(|e| format!("registry: {e}"))?;

    // -- Exec
    registry
        .set_server_enabled("k8s", false)
        .map_err(|e| format!("disable k8s: {e}"))?;
    let visible = visible_names(&state, &registry);

    // -- Check
    assert!(
        visible.contains(&"read".to_string()),
        "non-MCP tools must stay visible, got: {visible:?}"
    );
    assert!(
        visible.contains(&"gh__list_prs".to_string()),
        "tools from an enabled server must stay visible, got: {visible:?}"
    );
    assert!(
        !visible.contains(&"k8s__list_pods".to_string()),
        "tools from a disabled server must be hidden, got: {visible:?}"
    );
    assert!(
        registry.is_registered("k8s__list_pods"),
        "a disabled server's tool must stay registered"
    );
    assert!(
        !registry.contains("k8s__list_pods"),
        "a disabled server's tool must not be callable"
    );
    Ok(())
}

#[test]
fn mcp_registry_reenabling_server_restores_visibility() -> Result<()> {
    // -- Setup & Fixtures
    let state = tool_state(&["k8s__list_pods"])?;
    let registry = McpToolRegistry::from_tools(vec![mcp_tool("k8s", "list_pods")])
        .map_err(|e| format!("registry: {e}"))?;

    // -- Exec & Check
    registry
        .set_server_enabled("k8s", false)
        .map_err(|e| format!("disable k8s: {e}"))?;
    assert!(
        visible_names(&state, &registry).is_empty(),
        "a disabled server's tools must be hidden"
    );

    registry
        .set_server_enabled("k8s", true)
        .map_err(|e| format!("enable k8s: {e}"))?;
    assert_eq!(
        visible_names(&state, &registry),
        vec!["k8s__list_pods".to_string()],
        "re-enabling the server must restore visibility"
    );
    Ok(())
}

#[test]
fn mcp_registry_empty_tool_list_yields_no_active_mcp_definitions() -> Result<()> {
    // -- Setup & Fixtures
    let registry = McpToolRegistry::from_tools(Vec::new()).map_err(|e| format!("registry: {e}"))?;
    let empty_state = tool_state(&[])?;

    // -- Exec & Check
    assert!(
        visible_names(&empty_state, &registry).is_empty(),
        "an empty registry over an empty definition set must expose nothing"
    );
    assert!(
        !registry.is_registered("gh__list_prs"),
        "an empty registry must register no names"
    );
    assert!(
        !registry.is_server_enabled("gh"),
        "an empty registry must enable no servers"
    );

    // A definition the registry does not know is not MCP-filtered.
    let state = tool_state(&["read"])?;
    assert_eq!(
        visible_names(&state, &registry),
        vec!["read".to_string()],
        "an empty registry must not filter non-MCP definitions"
    );
    Ok(())
}

#[test]
fn mcp_registry_unknown_server_toggle_does_not_affect_known_servers() -> Result<()> {
    // -- Setup & Fixtures
    let state = tool_state(&["gh__list_prs"])?;
    let registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "list_prs")])
        .map_err(|e| format!("registry: {e}"))?;

    // -- Exec
    registry
        .set_server_enabled("not-configured", false)
        .map_err(|e| format!("disable unknown server: {e}"))?;

    // -- Check
    assert!(
        !registry.is_server_enabled("not-configured"),
        "an unknown server must not be reported as enabled"
    );
    assert!(
        registry.is_tool_enabled("gh__list_prs"),
        "an unknown server toggle must not disable a known server's tools"
    );
    assert_eq!(
        visible_names(&state, &registry),
        vec!["gh__list_prs".to_string()]
    );
    Ok(())
}

#[test]
fn mcp_registry_duplicate_exposed_name_is_rejected() -> Result<()> {
    // -- Setup & Fixtures
    // Two servers advertise the same exposed name — the registry must reject it.
    let tools = vec![
        McpToolDefinition {
            server: "gh".to_string(),
            name: "shared__tool".to_string(),
            raw_name: "tool".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "k8s".to_string(),
            name: "shared__tool".to_string(),
            raw_name: "tool".to_string(),
            description: None,
            parameters: None,
        },
    ];

    // -- Exec
    let result = McpToolRegistry::from_tools(tools);

    // -- Check
    let error = result.err().ok_or("a duplicate exposed name must fail")?;
    assert!(
        error.contains("duplicate exposed MCP tool name 'shared__tool'"),
        "got: {error}"
    );
    Ok(())
}

#[test]
fn mcp_registry_register_tools_conflicting_raw_mapping_is_rejected() -> Result<()> {
    // -- Setup & Fixtures
    let mut registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "list_prs")])
        .map_err(|e| format!("registry: {e}"))?;
    let conflicting = McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_all_prs".to_string(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let result = registry.register_tools(vec![conflicting]);

    // -- Check
    let error = result.err().ok_or("a conflicting raw mapping must fail")?;
    assert!(
        error.contains("conflicting raw MCP tool mapping for 'gh__list_prs'"),
        "got: {error}"
    );
    assert_eq!(
        registry.raw_name_for("gh__list_prs"),
        Some("list_prs"),
        "a rejected registration must leave the existing mapping intact"
    );
    Ok(())
}

#[test]
fn mcp_registry_register_tools_conflicting_owner_is_rejected() -> Result<()> {
    // -- Setup & Fixtures
    let mut registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "list_prs")])
        .map_err(|e| format!("registry: {e}"))?;
    let conflicting = McpToolDefinition {
        server: "k8s".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let result = registry.register_tools(vec![conflicting]);

    // -- Check
    let error = result.err().ok_or("a conflicting owner must fail")?;
    assert!(
        error.contains("conflicting MCP tool owner for 'gh__list_prs'"),
        "got: {error}"
    );
    assert_eq!(
        registry.server_name_for("gh__list_prs"),
        Some("gh"),
        "a rejected registration must leave the existing owner intact"
    );
    Ok(())
}

#[test]
fn mcp_registry_register_tools_is_idempotent_for_identical_tools() -> Result<()> {
    // -- Setup & Fixtures
    let mut registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "list_prs")])
        .map_err(|e| format!("registry: {e}"))?;

    // -- Exec
    registry
        .register_tools(vec![mcp_tool("gh", "list_prs")])
        .map_err(|e| format!("re-registering an identical tool must succeed: {e}"))?;

    // -- Check
    assert_eq!(registry.raw_name_for("gh__list_prs"), Some("list_prs"));
    assert_eq!(registry.server_name_for("gh__list_prs"), Some("gh"));
    assert!(registry.contains("gh__list_prs"));
    Ok(())
}

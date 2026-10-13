use crate::tools::authz::PermissionsConfig;
use crate::tools::handler::McpToolRegistry;
use crate::types::{EmptyToolName, ToolDefinition, ToolName};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper: creates a `ToolDefinition` with a given name and minimal schema.
fn tool_def(name: &str) -> core::result::Result<ToolDefinition, EmptyToolName> {
    Ok(ToolDefinition {
        name: ToolName::new(name)?,
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type": "object"}),
    })
}

/// An empty MCP registry (no MCP tools).
fn empty_mcp_registry() -> McpToolRegistry {
    McpToolRegistry::empty()
}

#[test]
fn ui_display_hides_denied_tools() -> Result<()> {
    let tools = vec![tool_def("read")?, tool_def("shell")?, tool_def("edit")?];
    let registry = empty_mcp_registry();
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
shell = "deny"
"#,
    )
    .unwrap();
    let permissions = PermissionsConfig::from_toml(&value, true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    let names: Vec<&str> = visible.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"read"), "expected 'read' to be visible");
    assert!(names.contains(&"edit"), "expected 'edit' to be visible");
    assert!(!names.contains(&"shell"), "expected 'shell' to be hidden");
    Ok(())
}

#[test]
fn ui_display_shows_ask_tools() -> Result<()> {
    let tools = vec![tool_def("read")?, tool_def("shell")?];
    let registry = empty_mcp_registry();
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"#,
    )
    .unwrap();
    let permissions = PermissionsConfig::from_toml(&value, true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    assert_eq!(
        visible.len(),
        2,
        "both tools should be visible with global ask"
    );
    Ok(())
}

#[test]
fn ui_display_global_deny_hides_all_except_allowed() -> Result<()> {
    let tools = vec![tool_def("read")?, tool_def("shell")?, tool_def("edit")?];
    let registry = empty_mcp_registry();
    let value: toml::Value = toml::from_str(
        r#"
"*" = "deny"
read = "allow"
"#,
    )
    .unwrap();
    let permissions = PermissionsConfig::from_toml(&value, true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    let names: Vec<&str> = visible.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["read"], "only 'read' should be visible");
    Ok(())
}

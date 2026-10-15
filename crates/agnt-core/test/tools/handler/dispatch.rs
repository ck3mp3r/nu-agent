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
fn ui_display_visibility_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, Vec<&str>, Vec<&str>)> = vec![
        (
            "hides denied tools",
            r#"
"*" = "ask"
shell = "deny"
"#,
            vec!["read", "shell", "edit"],
            vec!["read", "edit"],
        ),
        (
            "shows ask tools",
            r#"
"*" = "ask"
"#,
            vec!["read", "shell"],
            vec!["read", "shell"],
        ),
        (
            "global deny hides all except allowed",
            r#"
"*" = "deny"
read = "allow"
"#,
            vec!["read", "shell", "edit"],
            vec!["read"],
        ),
    ];

    // -- Exec & Check
    for (name, toml_src, tool_names, expected) in cases {
        let tools: Vec<ToolDefinition> = tool_names
            .iter()
            .map(|tool_name| tool_def(tool_name))
            .collect::<core::result::Result<_, _>>()?;
        let registry = empty_mcp_registry();
        let value: toml::Value = toml::from_str(toml_src)?;
        let permissions = PermissionsConfig::from_toml(&value, true);

        let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);
        let names: Vec<&str> = visible.iter().map(|t| t.name.as_str()).collect();

        assert_eq!(names, expected, "case '{name}': visible tool names");
    }
    Ok(())
}

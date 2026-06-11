use crate::agent::tools::authz::PermissionsConfig;
use crate::agent::tools::handler::McpToolRegistry;
use nu_protocol::{Value, record};

/// Helper: creates a `ToolDefinition` with a given name and minimal schema.
fn tool_def(name: &str) -> rig::completion::ToolDefinition {
    rig::completion::ToolDefinition {
        name: name.to_string(),
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type": "object"}),
    }
}

/// An empty MCP registry (no MCP tools).
fn empty_mcp_registry() -> McpToolRegistry {
    McpToolRegistry::from_names(Vec::<String>::new())
}

#[test]
fn ui_display_hides_denied_tools() {
    let tools = vec![tool_def("read"), tool_def("nu__run"), tool_def("edit")];
    let registry = empty_mcp_registry();
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
            "nu__run" => Value::test_string("deny"),
        })
    });
    let permissions = PermissionsConfig::parse_from_plugin_config(Some(&value), true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    let names: Vec<&str> = visible.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"read"), "expected 'read' to be visible");
    assert!(names.contains(&"edit"), "expected 'edit' to be visible");
    assert!(
        !names.contains(&"nu__run"),
        "expected 'nu__run' to be hidden"
    );
}

#[test]
fn ui_display_shows_ask_tools() {
    let tools = vec![tool_def("read"), tool_def("nu__run")];
    let registry = empty_mcp_registry();
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("ask"),
        })
    });
    let permissions = PermissionsConfig::parse_from_plugin_config(Some(&value), true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    assert_eq!(
        visible.len(),
        2,
        "both tools should be visible with global ask"
    );
}

#[test]
fn ui_display_global_deny_hides_all_except_allowed() {
    let tools = vec![tool_def("read"), tool_def("nu__run"), tool_def("edit")];
    let registry = empty_mcp_registry();
    let value = Value::test_record(record! {
        "permissions" => Value::test_record(record! {
            "*" => Value::test_string("deny"),
            "read" => Value::test_string("allow"),
        })
    });
    let permissions = PermissionsConfig::parse_from_plugin_config(Some(&value), true);

    let visible = super::llm_visible_tool_definitions(&tools, &registry, &permissions);

    let names: Vec<&str> = visible.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["read"], "only 'read' should be visible");
}

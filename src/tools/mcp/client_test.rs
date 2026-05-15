use super::{McpToolDefinition, filter_tools};

#[test]
fn filter_tools_matches_namespaced_server_only() {
    let tools = vec![
        McpToolDefinition::test_tool_with_raw("gh", "gh__list_prs", "list_prs"),
        McpToolDefinition::test_tool_with_raw("altgh", "altgh__list_prs", "list_prs"),
    ];

    let patterns = vec!["gh__*".to_string()];
    let selected = filter_tools(&tools, &patterns);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "gh__list_prs");
}

#[test]
fn filter_tools_matches_glob_pattern() {
    let tools = vec![
        McpToolDefinition::test_tool("server1", "read_file"),
        McpToolDefinition::test_tool("server1", "write_file"),
        McpToolDefinition::test_tool("server1", "delete_file"),
    ];
    let patterns = vec!["read_*".to_string(), "write_*".to_string()];
    let filtered = filter_tools(&tools, &patterns);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filter_tools_matches_multiple_patterns() {
    let tools = vec![
        McpToolDefinition::test_tool("server1", "foo"),
        McpToolDefinition::test_tool("server1", "bar"),
        McpToolDefinition::test_tool("server1", "baz"),
    ];
    let patterns = vec!["foo".to_string(), "bar".to_string()];
    let filtered = filter_tools(&tools, &patterns);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filter_tools_empty_patterns_returns_none() {
    let tools = vec![
        McpToolDefinition::test_tool("server1", "read"),
        McpToolDefinition::test_tool("server1", "write"),
    ];
    let patterns = vec![];
    let filtered = filter_tools(&tools, &patterns);
    assert_eq!(filtered.len(), 2); // Empty patterns match all
}

#[test]
fn filter_tools_matches_all_when_patterns_empty() {
    let tools = vec![
        McpToolDefinition::test_tool("s1", "k8s__list_pods"),
        McpToolDefinition::test_tool("s1", "gh__list_prs"),
    ];

    let tools = filter_tools(&tools, &[]);
    assert_eq!(tools.len(), 2);
}

#[test]
fn filter_tools_applies_glob_filters() {
    let tools = vec![
        McpToolDefinition::test_tool("s1", "k8s__list_pods"),
        McpToolDefinition::test_tool("s1", "gh__list_prs"),
    ];

    let patterns = vec!["gh__*".to_string()];
    let tools = filter_tools(&tools, &patterns);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "gh__list_prs");
}

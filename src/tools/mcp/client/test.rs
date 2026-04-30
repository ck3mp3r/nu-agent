use super::{McpToolDefinition, filter_tools};

#[test]
fn filter_tools_matches_all_when_patterns_empty() {
    let tools = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "k8s/list_pods".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/list_prs".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = filter_tools(&tools, &[]);
    assert_eq!(tools.len(), 2);
}

#[test]
fn filter_tools_applies_glob_filters() {
    let tools = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "k8s/list_pods".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/list_prs".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let patterns = vec!["gh/*".to_string()];
    let tools = filter_tools(&tools, &patterns);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "gh/list_prs");
}

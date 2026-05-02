use super::{McpToolDefinition, filter_tools};

#[test]
fn filter_tools_matches_all_when_patterns_empty() {
    let tools = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "k8s__list_pods".to_string(),
            raw_name: "list_pods".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
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
            name: "k8s__list_pods".to_string(),
            raw_name: "list_pods".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let patterns = vec!["gh__*".to_string()];
    let tools = filter_tools(&tools, &patterns);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "gh__list_prs");
}

#[test]
fn filter_tools_matches_namespaced_server_only() {
    let tools = vec![
        McpToolDefinition {
            server: "gh".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "altgh".to_string(),
            name: "altgh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let patterns = vec!["gh__*".to_string()];
    let selected = filter_tools(&tools, &patterns);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "gh__list_prs");
}

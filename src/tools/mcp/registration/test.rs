use crate::tools::mcp::client::McpToolDefinition;

use super::registerable_tools;

#[test]
fn registers_only_configured_and_runtime_allowed_tools() {
    let discovered = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/get_pr".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = registerable_tools(&discovered, &["gh/list_*".to_string()]);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "gh/list_prs");
}

#[test]
fn applies_cli_intersection_over_discovered_tools() {
    let discovered = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "k8s/list_pods".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = registerable_tools(&discovered, &["k8s/*".to_string()]);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "k8s/list_pods");
}

use crate::tools::mcp::client::McpToolDefinition;

use super::registerable_tools;

#[test]
fn registers_only_configured_and_runtime_allowed_tools() {
    let discovered = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh::list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh::get_pr".to_string(),
            raw_name: "get_pr".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = registerable_tools(&discovered, &["gh::list_*".to_string()]);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "gh::list_prs");
}

#[test]
fn applies_cli_intersection_over_discovered_tools() {
    let discovered = vec![
        McpToolDefinition {
            server: "s1".to_string(),
            name: "gh::list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "s1".to_string(),
            name: "k8s::list_pods".to_string(),
            raw_name: "list_pods".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = registerable_tools(&discovered, &["k8s::*".to_string()]);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "k8s::list_pods");
}

#[test]
fn keeps_same_raw_tool_name_distinct_when_servers_differ() {
    let discovered = vec![
        McpToolDefinition {
            server: "gh".to_string(),
            name: "gh::list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "altgh".to_string(),
            name: "altgh::list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
    ];

    let tools = registerable_tools(&discovered, &["*::*".to_string()]);
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().any(|t| t.name == "gh::list_prs"));
    assert!(tools.iter().any(|t| t.name == "altgh::list_prs"));
}

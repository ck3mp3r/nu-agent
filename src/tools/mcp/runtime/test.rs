use crate::tools::mcp::{client::McpToolDefinition, transport::McpTransportSpec};

use super::{McpRuntime, build_http_transport_config};

#[test]
fn discovered_tools_accessor_returns_runtime_tools() {
    let runtime = McpRuntime {
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        sessions: vec![],
        discovered_tools: vec![McpToolDefinition {
            server: "s1".to_string(),
            name: "gh::list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        }],
    };

    assert_eq!(runtime.discovered_tools().len(), 1);
    assert_eq!(runtime.discovered_tools()[0].name, "gh::list_prs");
}

#[test]
fn sse_transport_config_is_stateless() {
    let spec = McpTransportSpec::Sse {
        url: "https://example.com/mcp/sse".to_string(),
        headers: Default::default(),
    };

    let config = build_http_transport_config(&spec).expect("config");
    assert!(config.allow_stateless);
}

#[test]
fn http_transport_config_requires_session() {
    let spec = McpTransportSpec::Http {
        url: "https://example.com/mcp".to_string(),
        headers: Default::default(),
    };

    let config = build_http_transport_config(&spec).expect("config");
    assert!(!config.allow_stateless);
}

#[test]
fn compose_exposed_tool_name_prefixes_server_key() {
    let exposed = super::compose_exposed_tool_name("gh", "list_prs");
    assert_eq!(exposed, "gh::list_prs");
}

#[test]
fn compose_exposed_tool_name_prevents_cross_server_collisions() {
    let gh = super::compose_exposed_tool_name("gh", "list_prs");
    let alt = super::compose_exposed_tool_name("altgh", "list_prs");

    assert_ne!(gh, alt);
    assert_eq!(gh, "gh::list_prs");
    assert_eq!(alt, "altgh::list_prs");
}

#[test]
fn compose_exposed_tool_name_uses_reserved_delimiter() {
    let exposed = super::compose_exposed_tool_name("gh", "list_prs");
    assert!(exposed.contains("::"));
}

#[test]
fn register_exposed_name_fails_fast_on_duplicate_name() {
    let mut owners = std::collections::HashMap::new();
    super::register_exposed_name(&mut owners, "gh::list_prs", "gh").expect("first insert");

    let err = super::register_exposed_name(&mut owners, "gh::list_prs", "other")
        .expect_err("duplicate should fail");

    assert!(
        err.contains("duplicate exposed MCP tool name 'gh::list_prs'"),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_raw_tool_name_rejects_reserved_delimiter() {
    let err = super::validate_raw_tool_name("k8s", "list::pods")
        .expect_err("reserved delimiter should fail");

    assert!(
        err.contains("advertised tool 'list::pods' containing reserved delimiter '::'"),
        "unexpected error: {err}"
    );
}

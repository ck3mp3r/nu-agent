use crate::tools::mcp::{client::McpToolDefinition, transport::McpTransportSpec};

use super::{McpRuntime, build_http_transport_config};

#[test]
fn discovered_tools_accessor_returns_runtime_tools() {
    let runtime = McpRuntime {
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        sessions: vec![],
        discovered_tools: vec![McpToolDefinition {
            server: "s1".to_string(),
            name: "gh/list_prs".to_string(),
            description: None,
            parameters: None,
        }],
    };

    assert_eq!(runtime.discovered_tools().len(), 1);
    assert_eq!(runtime.discovered_tools()[0].name, "gh/list_prs");
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

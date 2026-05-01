use super::{McpTransportSpec, build_transport_spec};
use crate::tools::mcp::config::{McpServerConfig, McpTransportType};

#[test]
fn builds_stdio_transport_spec() {
    let server = McpServerConfig {
        name: "local".to_string(),
        transport: McpTransportType::Stdio,
        url: None,
        headers: Default::default(),
        command: Some("npx".to_string()),
        cwd: Some("/tmp".to_string()),
        args: vec!["-y".to_string(), "server".to_string()],
        env: [("FOO".to_string(), "BAR".to_string())]
            .into_iter()
            .collect(),
    };

    let spec = build_transport_spec(&server).expect("spec");
    match spec {
        McpTransportSpec::Stdio {
            command,
            cwd,
            args,
            env,
        } => {
            assert_eq!(command, "npx");
            assert_eq!(cwd.as_deref(), Some("/tmp"));
            assert_eq!(args, vec!["-y", "server"]);
            assert_eq!(env.get("FOO").map(String::as_str), Some("BAR"));
        }
        _ => panic!("expected stdio"),
    }
}

#[test]
fn builds_sse_transport_spec() {
    let server = McpServerConfig {
        name: "remote".to_string(),
        transport: McpTransportType::Sse,
        url: Some("https://example.com/mcp/sse".to_string()),
        headers: [("Authorization".to_string(), "Bearer x".to_string())]
            .into_iter()
            .collect(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    };

    let spec = build_transport_spec(&server).expect("spec");
    match spec {
        McpTransportSpec::Sse { url, headers } => {
            assert_eq!(url, "https://example.com/mcp/sse");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer x")
            );
        }
        _ => panic!("expected sse"),
    }
}

#[test]
fn builds_http_transport_spec_for_streamable_http() {
    let server = McpServerConfig {
        name: "remote".to_string(),
        transport: McpTransportType::StreamableHttp,
        url: Some("https://example.com/mcp".to_string()),
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
    };

    let spec = build_transport_spec(&server).expect("spec");
    match spec {
        McpTransportSpec::Http { url, .. } => {
            assert_eq!(url, "https://example.com/mcp");
        }
        _ => panic!("expected http"),
    }
}

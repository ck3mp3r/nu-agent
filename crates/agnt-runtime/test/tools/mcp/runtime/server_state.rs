use super::*;

#[test]
fn discovered_tools_accessor_returns_runtime_tools() {
    let runtime = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::new(),
        discovered_tools: vec![McpToolDefinition {
            server: "s1".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        }],
    };

    assert_eq!(runtime.discovered_tools().len(), 1);
    assert_eq!(runtime.discovered_tools()[0].name, "gh__list_prs");
}

#[test]
fn lifecycle_projection_table() {
    // -- Setup & Fixtures
    fn server(
        name: &str,
        transport: McpTransportType,
        url: &str,
        enabled: bool,
    ) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            transport,
            url: Some(url.to_string()),
            headers: Default::default(),
            auth: McpAuthConfig::None,
            command: None,
            cwd: None,
            args: vec![],
            env: Default::default(),
            enabled,
        }
    }

    struct Case {
        name: &'static str,
        servers: Vec<McpServerConfig>,
        connected: std::collections::BTreeSet<String>,
        discovered_tools: Vec<McpToolDefinition>,
        expected: Vec<(&'static str, bool, bool, bool, usize)>,
    }

    let cases = vec![
        Case {
            name: "reports all configured servers with deterministic fields",
            servers: vec![
                server(
                    "enabled-server",
                    McpTransportType::Sse,
                    "https://example.com/mcp/sse",
                    true,
                ),
                server(
                    "disabled-server",
                    McpTransportType::Http,
                    "https://example.com/mcp/http",
                    false,
                ),
            ],
            connected: std::collections::BTreeSet::new(),
            discovered_tools: vec![],
            expected: vec![
                ("enabled-server", true, true, false, 0),
                ("disabled-server", true, false, false, 0),
            ],
        },
        Case {
            name: "marks connected when a runtime session exists for the server",
            servers: vec![server(
                "connected-server",
                McpTransportType::Sse,
                "https://example.com/mcp/sse",
                true,
            )],
            connected: std::collections::BTreeSet::from(["connected-server".to_string()]),
            discovered_tools: vec![McpToolDefinition {
                server: "connected-server".to_string(),
                name: "connected-server__list".to_string(),
                raw_name: "list".to_string(),
                description: None,
                parameters: None,
            }],
            expected: vec![("connected-server", true, true, true, 0)],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let runtime = McpRuntime {
            sessions: vec![],
            connected_servers: case.connected,
            discovered_tools: case.discovered_tools,
        };

        let projection = runtime.lifecycle_projection(&case.servers);
        assert_eq!(
            projection.len(),
            case.expected.len(),
            "case '{}': projection len",
            case.name
        );
        for (name, configured, enabled, connected, visible_tool_count) in &case.expected {
            let entry = projection
                .iter()
                .find(|s| s.name == *name)
                .expect("server present in projection");
            assert_eq!(
                entry.configured, *configured,
                "case '{}': {name} configured",
                case.name
            );
            assert_eq!(
                entry.enabled, *enabled,
                "case '{}': {name} enabled",
                case.name
            );
            assert_eq!(
                entry.connected, *connected,
                "case '{}': {name} connected",
                case.name
            );
            assert_eq!(
                entry.visible_tool_count, *visible_tool_count,
                "case '{}': {name} visible_tool_count",
                case.name
            );
        }
    }
}

#[test]
fn mark_disconnected_removes_from_connected_servers() {
    let servers = vec![McpServerConfig {
        name: "my-server".to_string(),
        transport: McpTransportType::Sse,
        url: Some("https://example.com/mcp/sse".to_string()),
        headers: Default::default(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
        enabled: true,
    }];

    let mut runtime = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["my-server".to_string()]),

        discovered_tools: vec![],
    };

    assert!(
        runtime.has_server("my-server"),
        "should be connected before mark_disconnected"
    );
    assert!(runtime.lifecycle_projection(&servers)[0].connected);

    runtime.mark_disconnected("my-server");

    assert!(
        !runtime.has_server("my-server"),
        "should not be connected after mark_disconnected"
    );
    assert!(!runtime.lifecycle_projection(&servers)[0].connected);
}

#[test]
fn activation_gating_selects_only_enabled_servers() {
    let servers = vec![
        McpServerConfig {
            name: "enabled-a".to_string(),
            transport: McpTransportType::Sse,
            url: Some("https://example.com/mcp/sse".to_string()),
            headers: Default::default(),
            auth: McpAuthConfig::None,
            command: None,
            cwd: None,
            args: vec![],
            env: Default::default(),
            enabled: true,
        },
        McpServerConfig {
            name: "disabled-b".to_string(),
            transport: McpTransportType::Http,
            url: Some("https://example.com/mcp/http".to_string()),
            headers: Default::default(),
            auth: McpAuthConfig::None,
            command: None,
            cwd: None,
            args: vec![],
            env: Default::default(),
            enabled: false,
        },
    ];

    let selected = super::select_enabled_servers(&servers);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "enabled-a");
}

#[test]
fn transport_config_statelessness_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, McpTransportType, &str, bool)> = vec![
        (
            "sse is stateless",
            McpTransportType::Sse,
            "https://example.com/mcp/sse",
            true,
        ),
        (
            "http requires a session",
            McpTransportType::Http,
            "https://example.com/mcp",
            false,
        ),
    ];

    // -- Exec & Check
    for (name, transport, url, expected) in cases {
        let server = McpServerConfig {
            name: "srv".to_string(),
            transport,
            url: Some(url.to_string()),
            headers: Default::default(),
            auth: McpAuthConfig::None,
            command: None,
            cwd: None,
            args: vec![],
            env: Default::default(),
            enabled: true,
        };
        let config = build_http_transport_config(&server, None).expect("config");
        assert_eq!(config.allow_stateless, expected, "case '{name}'");
    }
}

/// An `McpToolDefinition` for `server` exposing `raw`.
fn discovered(server: &str, raw: &str) -> McpToolDefinition {
    McpToolDefinition {
        server: server.to_string(),
        name: format!("{server}__{raw}"),
        raw_name: raw.to_string(),
        description: None,
        parameters: None,
    }
}

#[test]
fn merge_combines_connected_servers_and_discovered_tools() {
    // -- Setup & Fixtures
    let mut base = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["gh".to_string()]),
        discovered_tools: vec![discovered("gh", "list_prs")],
    };
    let other = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["k8s".to_string()]),
        discovered_tools: vec![discovered("k8s", "list_pods")],
    };

    // -- Exec
    base.merge(other);

    // -- Check
    assert!(
        base.has_server("gh") && base.has_server("k8s"),
        "merge must keep the existing server and add the new one"
    );
    let names: Vec<&str> = base
        .discovered_tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(names, vec!["gh__list_prs", "k8s__list_pods"]);
}

#[test]
fn merge_keeps_existing_state_when_other_is_empty() {
    // -- Setup & Fixtures
    let mut base = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["gh".to_string()]),
        discovered_tools: vec![discovered("gh", "list_prs")],
    };
    let empty = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::new(),
        discovered_tools: vec![],
    };

    // -- Exec
    base.merge(empty);

    // -- Check
    assert!(
        base.has_server("gh"),
        "merge must not drop existing servers"
    );
    assert_eq!(base.discovered_tools().len(), 1);
}

#[test]
fn merge_after_mark_disconnected_restores_the_server() {
    // -- Setup & Fixtures
    let mut base = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["gh".to_string()]),
        discovered_tools: vec![],
    };
    let reconnected = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["gh".to_string()]),
        discovered_tools: vec![],
    };

    // -- Exec
    base.mark_disconnected("gh");
    let after_disconnect = base.has_server("gh");
    base.merge(reconnected);

    // -- Check
    assert!(!after_disconnect, "mark_disconnected must clear the server");
    assert!(
        base.has_server("gh"),
        "merging a reconnected runtime must restore the server"
    );
}

use super::*;

// ── build_http_transport_config auth wiring ─────────────────────────────────

fn server_with(
    name: &str,
    transport: McpTransportType,
    url: &str,
    headers: HashMap<String, String>,
    auth: McpAuthConfig,
) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport,
        url: Some(url.to_string()),
        headers,
        auth,
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    }
}

#[test]
fn http_auth_header_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, McpServerConfig, Option<&str>)> = vec![
        (
            "bearer sets the auth header",
            server_with(
                "bearer-test",
                McpTransportType::Http,
                "https://api.example.com/mcp",
                HashMap::new(),
                McpAuthConfig::Bearer {
                    token: "my-token".to_string(),
                },
            ),
            Some("my-token"),
        ),
        (
            "none leaves the auth header unset",
            server_with(
                "none-auth",
                McpTransportType::Http,
                "https://api.example.com/mcp",
                HashMap::new(),
                McpAuthConfig::None,
            ),
            None,
        ),
        (
            "oauth leaves the auth header unset",
            server_with(
                "oauth-test",
                McpTransportType::Http,
                "https://api.example.com/mcp",
                HashMap::new(),
                McpAuthConfig::OAuth {
                    client_id: None,
                    client_secret: None,
                    scope: None,
                    redirect_uri: None,
                },
            ),
            None,
        ),
        (
            "sse bearer sets the auth header",
            server_with(
                "sse-bearer",
                McpTransportType::Sse,
                "https://api.example.com/mcp/sse",
                HashMap::new(),
                McpAuthConfig::Bearer {
                    token: "sse-token".to_string(),
                },
            ),
            Some("sse-token"),
        ),
    ];

    // -- Exec & Check
    for (name, server, expected) in cases {
        let config = build_http_transport_config(&server, None).expect("config");
        assert_eq!(
            config.auth_header.as_deref(),
            expected,
            "case '{name}': auth header"
        );
    }
}

#[test]
fn http_bearer_auth_skips_authorization_from_custom_headers() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer old-token".to_string());
    headers.insert("X-API-Key".to_string(), "abc123".to_string());

    let server = McpServerConfig {
        name: "bearer-skip".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers,
        auth: McpAuthConfig::Bearer {
            token: "new-token".to_string(),
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server, None).expect("config");
    // auth_header is set from the auth field
    assert_eq!(config.auth_header, Some("new-token".to_string()));
    // Authorization header is NOT in custom_headers
    assert!(
        !config
            .custom_headers
            .contains_key(&HeaderName::from_static("authorization"))
    );
    // Non-auth headers still pass through
    assert!(
        config
            .custom_headers
            .contains_key(&HeaderName::from_static("x-api-key"))
    );
}

#[test]
fn http_custom_header_passthrough_table() {
    // -- Setup & Fixtures
    let mut none_headers = HashMap::new();
    none_headers.insert("Authorization".to_string(), "Bearer old-token".to_string());

    let mut custom_headers = HashMap::new();
    custom_headers.insert("X-Custom".to_string(), "value1".to_string());
    custom_headers.insert("X-Request-Id".to_string(), "req-123".to_string());

    type Case = (&'static str, McpServerConfig, Vec<(&'static str, bool)>);

    let cases: Vec<Case> = vec![
        (
            "none auth passes the authorization header through",
            server_with(
                "none-pass",
                McpTransportType::Http,
                "https://api.example.com/mcp",
                none_headers,
                McpAuthConfig::None,
            ),
            vec![("authorization", true)],
        ),
        (
            "non-auth headers always pass through",
            server_with(
                "custom-headers",
                McpTransportType::Http,
                "https://api.example.com/mcp",
                custom_headers,
                McpAuthConfig::Bearer {
                    token: "tok".to_string(),
                },
            ),
            vec![("x-custom", true), ("x-request-id", true)],
        ),
    ];

    // -- Exec & Check
    for (name, server, checks) in cases {
        let config = build_http_transport_config(&server, None).expect("config");
        for (header, expected) in checks {
            assert_eq!(
                config
                    .custom_headers
                    .contains_key(&HeaderName::from_static(header)),
                expected,
                "case '{name}': custom header {header}"
            );
        }
    }
}

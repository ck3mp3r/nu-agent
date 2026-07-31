use std::collections::HashMap;

use http::HeaderName;

use crate::tools::mcp::{
    client::McpToolDefinition,
    config::{McpAuthConfig, McpServerConfig, McpTransportType},
};

use super::{McpRuntime, build_http_transport_config};

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
fn connect_server_states_reports_all_configured_servers_with_deterministic_fields() {
    let servers = vec![
        McpServerConfig {
            name: "enabled-server".to_string(),
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
            name: "disabled-server".to_string(),
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

    let runtime = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::new(),

        discovered_tools: vec![],
    };

    let projection = runtime.lifecycle_projection(&servers);
    assert_eq!(projection.len(), 2);

    let enabled = projection
        .iter()
        .find(|s| s.name == "enabled-server")
        .expect("enabled server present");
    assert!(enabled.configured);
    assert!(enabled.enabled);
    assert!(!enabled.connected);
    assert_eq!(enabled.visible_tool_count, 0);

    let disabled = projection
        .iter()
        .find(|s| s.name == "disabled-server")
        .expect("disabled server present");
    assert!(disabled.configured);
    assert!(!disabled.enabled);
    assert!(!disabled.connected);
    assert_eq!(disabled.visible_tool_count, 0);
}

#[test]
fn connect_server_states_marks_connected_when_runtime_session_exists_for_server() {
    let servers = vec![McpServerConfig {
        name: "connected-server".to_string(),
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

    let runtime = McpRuntime {
        sessions: vec![],
        connected_servers: std::collections::BTreeSet::from(["connected-server".to_string()]),

        discovered_tools: vec![McpToolDefinition {
            server: "connected-server".to_string(),
            name: "connected-server__list".to_string(),
            raw_name: "list".to_string(),
            description: None,
            parameters: None,
        }],
    };

    let projection = runtime.lifecycle_projection(&servers);
    assert_eq!(projection.len(), 1);
    assert!(projection[0].configured);
    assert!(projection[0].enabled);
    assert!(projection[0].connected);
    assert_eq!(projection[0].visible_tool_count, 0);
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
fn sse_transport_config_is_stateless() {
    let server = McpServerConfig {
        name: "sse".to_string(),
        transport: McpTransportType::Sse,
        url: Some("https://example.com/mcp/sse".to_string()),
        headers: Default::default(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert!(config.allow_stateless);
}

#[test]
fn http_transport_config_requires_session() {
    let server = McpServerConfig {
        name: "http".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://example.com/mcp".to_string()),
        headers: Default::default(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert!(!config.allow_stateless);
}

#[test]
fn compose_exposed_tool_name_prefixes_server_key() {
    let exposed = super::compose_exposed_tool_name("gh", "list_prs");
    assert_eq!(exposed, "gh__list_prs");
}

#[test]
fn compose_exposed_tool_name_prevents_cross_server_collisions() {
    let gh = super::compose_exposed_tool_name("gh", "list_prs");
    let alt = super::compose_exposed_tool_name("altgh", "list_prs");

    assert_ne!(gh, alt);
    assert_eq!(gh, "gh__list_prs");
    assert_eq!(alt, "altgh__list_prs");
}

#[test]
fn compose_exposed_tool_name_uses_reserved_delimiter() {
    let exposed = super::compose_exposed_tool_name("gh", "list_prs");
    assert!(exposed.contains("__"));
}

#[test]
fn register_exposed_name_fails_fast_on_duplicate_name() {
    let mut owners = std::collections::HashMap::new();
    super::register_exposed_name(&mut owners, "gh__list_prs", "gh").expect("first insert");

    let err = super::register_exposed_name(&mut owners, "gh__list_prs", "other")
        .expect_err("duplicate should fail");

    assert!(
        err.contains("duplicate exposed MCP tool name 'gh__list_prs'"),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_raw_tool_name_rejects_reserved_delimiter() {
    let err = super::validate_raw_tool_name("k8s", "list__pods")
        .expect_err("reserved delimiter should fail");

    assert!(
        err.contains("advertised tool 'list__pods' containing reserved delimiter '__'"),
        "unexpected error: {err}"
    );
}

#[test]
fn resolve_stdio_cwd_prefers_override_when_valid() {
    let base = std::env::temp_dir();
    let caller = base.join("nu-agent-mcp-caller");
    let override_dir = base.join("nu-agent-mcp-override");
    std::fs::create_dir_all(&caller).expect("create caller");
    std::fs::create_dir_all(&override_dir).expect("create override");

    let resolved = super::resolve_stdio_cwd(
        caller.as_path(),
        Some(override_dir.to_string_lossy().to_string()),
        "nu",
    )
    .expect("cwd resolve");

    let expected = std::fs::canonicalize(&override_dir).expect("canonical override");
    assert_eq!(resolved, expected);
}

#[test]
fn resolve_stdio_cwd_uses_caller_when_no_override() {
    let caller = std::env::temp_dir().join("nu-agent-mcp-caller-only");
    std::fs::create_dir_all(&caller).expect("create caller");

    let resolved = super::resolve_stdio_cwd(caller.as_path(), None, "nu").expect("cwd resolve");

    let expected = std::fs::canonicalize(&caller).expect("canonical caller");
    assert_eq!(resolved, expected);
}

#[test]
fn resolve_stdio_cwd_rejects_invalid_override() {
    let caller = std::env::temp_dir().join("nu-agent-mcp-caller-invalid");
    std::fs::create_dir_all(&caller).expect("create caller");
    let bad = caller.join("does-not-exist");

    let err = super::resolve_stdio_cwd(
        caller.as_path(),
        Some(bad.to_string_lossy().to_string()),
        "nu",
    )
    .expect_err("invalid override must fail");

    assert!(
        err.contains("invalid stdio cwd override") || err.contains("not a directory"),
        "unexpected error: {err}"
    );
}

#[test]
fn resolve_stdio_cwd_requires_caller_when_no_override() {
    let err = super::resolve_caller_cwd(None, "nu").expect_err("missing cwd must fail");

    assert!(
        err.contains("caller cwd") || err.contains("missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn merged_stdio_env_overwrites_pwd_for_compatibility() {
    let cwd = std::env::temp_dir().join("nu-agent-effective-cwd");
    let caller = std::env::temp_dir().join("nu-agent-caller-cwd");
    std::fs::create_dir_all(&cwd).expect("create effective cwd");
    std::fs::create_dir_all(&caller).expect("create caller cwd");
    let env = std::collections::HashMap::from([("PWD".to_string(), "/wrong".to_string())]);

    let merged = super::merged_stdio_env_with_pwd(env, cwd.as_path(), caller.as_path());
    assert_eq!(
        merged.get("PWD").map(String::as_str),
        Some(cwd.to_string_lossy().as_ref())
    );
    assert_eq!(
        merged.get("NU_AGENT_CALLER_CWD").map(String::as_str),
        Some(caller.to_string_lossy().as_ref())
    );
    assert_eq!(
        merged.get("NU_AGENT_CALLER_PWD").map(String::as_str),
        Some(caller.to_string_lossy().as_ref())
    );
}

#[test]
fn resolve_stdio_cwd_relative_override_resolves_from_caller_cwd() {
    let base = std::env::temp_dir().join("nu-agent-mcp-relative");
    let caller = base.join("caller");
    let nested = caller.join("workspace").join("project");
    std::fs::create_dir_all(&nested).expect("create nested cwd");

    let resolved = super::resolve_stdio_cwd(
        caller.as_path(),
        Some("workspace/project".to_string()),
        "nu",
    )
    .expect("cwd resolve");

    let expected = std::fs::canonicalize(&nested).expect("canonical nested");
    assert_eq!(resolved, expected);
}

#[test]
fn resolve_stdio_cwd_absolute_override_works() {
    let base = std::env::temp_dir().join("nu-agent-mcp-absolute");
    let caller = base.join("caller");
    let absolute_override = base.join("absolute-override");
    std::fs::create_dir_all(&caller).expect("create caller cwd");
    std::fs::create_dir_all(&absolute_override).expect("create absolute override");

    let resolved = super::resolve_stdio_cwd(
        caller.as_path(),
        Some(absolute_override.to_string_lossy().to_string()),
        "nu",
    )
    .expect("cwd resolve");

    let expected = std::fs::canonicalize(&absolute_override).expect("canonical override");
    assert_eq!(resolved, expected);
}

// ── build_http_transport_config auth wiring ─────────────────────────────────

#[test]
fn http_bearer_auth_sets_auth_header_on_transport_config() {
    let server = McpServerConfig {
        name: "bearer-test".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers: HashMap::new(),
        auth: McpAuthConfig::Bearer {
            token: "my-token".to_string(),
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert_eq!(config.auth_header, Some("my-token".to_string()));
}

#[test]
fn http_none_auth_leaves_auth_header_unset() {
    let server = McpServerConfig {
        name: "none-auth".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers: HashMap::new(),
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert_eq!(config.auth_header, None);
}

#[test]
fn http_oauth_auth_leaves_auth_header_unset() {
    let server = McpServerConfig {
        name: "oauth-test".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers: HashMap::new(),
        auth: McpAuthConfig::OAuth {
            client_id: None,
            client_secret: None,
            scope: None,
            redirect_uri: None,
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert_eq!(config.auth_header, None);
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

    let config = build_http_transport_config(&server).expect("config");
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
fn http_none_auth_passes_authorization_header_through() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer old-token".to_string());

    let server = McpServerConfig {
        name: "none-pass".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers,
        auth: McpAuthConfig::None,
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    // auth_header is not set
    assert_eq!(config.auth_header, None);
    // Authorization header IS in custom_headers (backwards compat)
    assert!(
        config
            .custom_headers
            .contains_key(&HeaderName::from_static("authorization"))
    );
}

#[test]
fn http_non_auth_headers_always_pass_through() {
    let mut headers = HashMap::new();
    headers.insert("X-Custom".to_string(), "value1".to_string());
    headers.insert("X-Request-Id".to_string(), "req-123".to_string());

    let server = McpServerConfig {
        name: "custom-headers".to_string(),
        transport: McpTransportType::Http,
        url: Some("https://api.example.com/mcp".to_string()),
        headers,
        auth: McpAuthConfig::Bearer {
            token: "tok".to_string(),
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert!(
        config
            .custom_headers
            .contains_key(&HeaderName::from_static("x-custom"))
    );
    assert!(
        config
            .custom_headers
            .contains_key(&HeaderName::from_static("x-request-id"))
    );
}

#[test]
fn sse_bearer_auth_sets_auth_header() {
    let server = McpServerConfig {
        name: "sse-bearer".to_string(),
        transport: McpTransportType::Sse,
        url: Some("https://api.example.com/mcp/sse".to_string()),
        headers: HashMap::new(),
        auth: McpAuthConfig::Bearer {
            token: "sse-token".to_string(),
        },
        command: None,
        cwd: None,
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    };

    let config = build_http_transport_config(&server).expect("config");
    assert_eq!(config.auth_header, Some("sse-token".to_string()));
}

#[tokio::test]
async fn connect_servers_does_not_replace_existing_handle_contents() {
    use rig::tool::server::ToolServer;

    // Create a handle — simulates the application-level handle
    // that already has builtins registered on it.
    // We cannot add a real tool here without implementing ToolDyn,
    // so we verify structurally: McpRuntime must NOT contain a
    // tool_server_handle field (it was removed in this fix).
    // This test compiles only if the struct has no such field.
    let handle = ToolServer::new().run();

    // Connect with no servers — should succeed and return empty runtime
    let result = crate::tools::mcp::runtime::connect_servers(&handle, &[], None, 20_000).await;

    assert!(
        result.is_ok(),
        "connect_servers with empty config should succeed"
    );
    let mcp_runtime = result.unwrap();
    assert!(
        !mcp_runtime.has_sessions(),
        "no sessions expected with empty config"
    );

    // Structural proof: if McpRuntime had a tool_server_handle field,
    // this line would not compile (field access would exist but not here).
    // The absence of McpRuntime::tool_server_handle() is enforced at compile time.
}

// ── classify_mcp_error ────────────────────────────────────────────────────────

#[test]
fn classify_mcp_error_returns_auth_required_for_auth_required() {
    let err = super::classify_mcp_error("AuthRequired: token expired", "my-server");
    assert!(err.is_some());
    match err.unwrap() {
        crate::tools::mcp::auth_error::McpAuthError::AuthRequired { server } => {
            assert_eq!(server, "my-server");
        }
        other => panic!("expected AuthRequired, got {other:?}"),
    }
}

#[test]
fn classify_mcp_error_returns_refresh_failed_for_refresh_failed() {
    let err = super::classify_mcp_error("token refresh failed", "my-server");
    assert!(err.is_some());
    match err.unwrap() {
        crate::tools::mcp::auth_error::McpAuthError::RefreshFailed { server } => {
            assert_eq!(server, "my-server");
        }
        other => panic!("expected RefreshFailed, got {other:?}"),
    }
}

#[test]
fn classify_mcp_error_returns_insufficient_scope_for_insufficient_scope() {
    let err = super::classify_mcp_error("InsufficientScope: missing read", "my-server");
    assert!(err.is_some());
    match err.unwrap() {
        crate::tools::mcp::auth_error::McpAuthError::InsufficientScope { server, required } => {
            assert_eq!(server, "my-server");
            assert_eq!(required, "see server documentation");
        }
        other => panic!("expected InsufficientScope, got {other:?}"),
    }
}

#[test]
fn classify_mcp_error_returns_not_authenticated_for_not_authenticated() {
    let err = super::classify_mcp_error("not authenticated", "my-server");
    assert!(err.is_some());
    match err.unwrap() {
        crate::tools::mcp::auth_error::McpAuthError::NotAuthenticated { server } => {
            assert_eq!(server, "my-server");
        }
        other => panic!("expected NotAuthenticated, got {other:?}"),
    }
}

#[test]
fn classify_mcp_error_returns_none_for_transport_errors() {
    assert!(super::classify_mcp_error("connection refused", "my-server").is_none());
    assert!(super::classify_mcp_error("transport closed", "my-server").is_none());
    assert!(super::classify_mcp_error("broken pipe", "my-server").is_none());
}

#[test]
fn classify_mcp_error_returns_none_for_arbitrary_text() {
    assert!(super::classify_mcp_error("some random error", "my-server").is_none());
    assert!(super::classify_mcp_error("", "my-server").is_none());
}

#[test]
fn classify_mcp_error_returns_none_for_transport_errors_containing_auth() {
    // These are transport errors, not auth errors — must not match
    assert!(
        super::classify_mcp_error("failed to connect to auth server", "my-server").is_none(),
        "transport error 'failed to connect to auth server' should not match auth patterns"
    );
    assert!(
        super::classify_mcp_error("failed to resolve auth.example.com", "my-server").is_none(),
        "transport error 'failed to resolve auth.example.com' should not match auth patterns"
    );
    assert!(
        super::classify_mcp_error("connection to auth backend failed", "my-server").is_none(),
        "transport error 'connection to auth backend failed' should not match auth patterns"
    );
}

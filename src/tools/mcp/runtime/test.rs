use crate::tools::mcp::{
    client::McpToolDefinition,
    config::{McpServerConfig, McpTransportType},
};

use super::{McpRuntime, build_http_transport_config};

#[test]
fn discovered_tools_accessor_returns_runtime_tools() {
    let runtime = McpRuntime {
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        sessions: vec![],
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
fn sse_transport_config_is_stateless() {
    let server = McpServerConfig {
        name: "sse".to_string(),
        transport: McpTransportType::Sse,
        url: Some("https://example.com/mcp/sse".to_string()),
        headers: Default::default(),
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
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
        command: None,
        cwd: None,
        args: vec![],
        env: Default::default(),
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

    let resolved =
        super::resolve_stdio_cwd(caller.as_path(), Some(override_dir.to_string_lossy().to_string()), "nu")
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

    let err =
        super::resolve_stdio_cwd(caller.as_path(), Some(bad.to_string_lossy().to_string()), "nu")
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

    let resolved =
        super::resolve_stdio_cwd(caller.as_path(), Some("workspace/project".to_string()), "nu")
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

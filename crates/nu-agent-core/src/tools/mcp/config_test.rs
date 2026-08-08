use super::{McpAuthConfig, McpConfig};

#[test]
fn mcp_config_from_toml_reads_map_shape() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
enabled = true

[nu]
transport = "stdio"
command = "nu-mcp"
cwd = "/tmp"
args = ["--add-path", "/tmp"]
enabled = false

[nu.env]
GIT_PAGER = ""
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse mcp config");
    assert_eq!(parsed.mcp.len(), 2);

    let c5t = parsed
        .mcp
        .iter()
        .find(|s| s.name == "c5t")
        .expect("c5t server exists");
    assert_eq!(c5t.url.as_deref(), Some("http://0.0.0.0:3737/mcp"));
    assert!(c5t.enabled);

    let nu = parsed
        .mcp
        .iter()
        .find(|s| s.name == "nu")
        .expect("nu server exists");
    assert_eq!(nu.command.as_deref(), Some("nu-mcp"));
    assert_eq!(nu.cwd.as_deref(), Some("/tmp"));
    assert_eq!(nu.args, vec!["--add-path".to_string(), "/tmp".to_string()]);
    assert_eq!(nu.env.get("GIT_PAGER").map(String::as_str), Some(""));
    assert!(!nu.enabled);
}

#[test]
fn mcp_config_enabled_defaults_true_when_omitted() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse mcp config");
    let c5t = parsed
        .mcp
        .iter()
        .find(|s| s.name == "c5t")
        .expect("c5t server exists");
    assert!(c5t.enabled);
}

#[test]
fn mcp_config_enabled_defaults_true_on_non_bool() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
enabled = "true"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let c5t = parsed.mcp.iter().find(|s| s.name == "c5t").unwrap();
    assert!(c5t.enabled);
}

#[test]
fn mcp_config_validation_rejects_empty_stdio_cwd_when_set() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[nu]
transport = "stdio"
command = "nu-mcp"
cwd = "   "
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("requires non-empty 'cwd'") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_config_from_toml_missing_mcp_returns_empty() {
    let mcp_value: toml::Value = toml::Value::Table(toml::value::Table::new());

    let parsed = McpConfig::from_toml(&mcp_value).expect("missing mcp should be ok");
    assert!(parsed.mcp.is_empty());
}

#[test]
fn mcp_config_from_toml_non_table_returns_empty() {
    let mcp_value: toml::Value = toml::Value::String("not a table".to_string());

    let parsed = McpConfig::from_toml(&mcp_value).expect("non-table should be empty");
    assert!(parsed.mcp.is_empty());
}

#[test]
fn mcp_config_validation_requires_command_for_stdio() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[nu]
transport = "stdio"
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("requires non-empty 'command'") || msg.contains("Invalid MCP configuration")
    );
}

#[test]
fn mcp_config_validation_requires_url_for_remote_transports() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("requires non-empty 'url'") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_config_rejects_unsupported_transport() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "unsupported"
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("unsupported transport") || msg.contains("Invalid transport"));
}

#[test]
fn mcp_config_rejects_server_name_with_reserved_delimiter() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[gh__prod]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("reserved delimiter") || msg.contains("Invalid MCP configuration"));
}

// ── McpAuthConfig tests ──────────────────────────────────────────────────────

#[test]
fn mcp_auth_defaults_to_none_when_omitted() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let c5t = parsed.mcp.iter().find(|s| s.name == "c5t").unwrap();
    assert_eq!(c5t.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_parses_none_explicitly() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"

[c5t.auth]
type = "none"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let c5t = parsed.mcp.iter().find(|s| s.name == "c5t").unwrap();
    assert_eq!(c5t.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_parses_bearer() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[api]
transport = "http"
url = "https://api.example.com/mcp"

[api.auth]
type = "bearer"
token = "abc123"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let api = parsed.mcp.iter().find(|s| s.name == "api").unwrap();
    assert_eq!(
        api.auth,
        McpAuthConfig::Bearer {
            token: "abc123".to_string()
        }
    );
}

#[test]
fn mcp_auth_parses_oauth_with_scope() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[ctx]
transport = "http"
url = "https://mcp.context7.com/mcp"

[ctx.auth]
type = "oauth"
scope = "profile email"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let ctx = parsed.mcp.iter().find(|s| s.name == "ctx").unwrap();
    assert_eq!(
        ctx.auth,
        McpAuthConfig::OAuth {
            client_id: None,
            client_secret: None,
            scope: Some("profile email".to_string()),
            redirect_uri: None,
        }
    );
}

#[test]
fn mcp_auth_parses_oauth_with_client_credentials() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[gh]
transport = "http"
url = "https://api.github.com/mcp"

[gh.auth]
type = "oauth"
client-id = "Iv1.example"
client-secret = "secret123"
scope = "repo read:org"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let gh = parsed.mcp.iter().find(|s| s.name == "gh").unwrap();
    assert_eq!(
        gh.auth,
        McpAuthConfig::OAuth {
            client_id: Some("Iv1.example".to_string()),
            client_secret: Some("secret123".to_string()),
            scope: Some("repo read:org".to_string()),
            redirect_uri: None,
        }
    );
}

#[test]
fn mcp_auth_unknown_type_defaults_to_none() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
type = "unknown"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let x = parsed.mcp.iter().find(|s| s.name == "x").unwrap();
    assert_eq!(x.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_rejects_bearer_with_empty_token() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
type = "bearer"
token = ""
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("empty token") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_auth_rejects_oauth_on_stdio() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "stdio"
command = "my-server"
cwd = "/tmp"

[x.auth]
type = "oauth"
scope = "profile"
"#,
    )
    .unwrap();

    let err = McpConfig::from_toml(&mcp_value).expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("requires HTTP or SSE transport") || msg.contains("Invalid MCP configuration")
    );
}

#[test]
fn mcp_auth_backwards_compat_headers_authorization_still_works() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[api]
transport = "http"
url = "https://api.example.com/mcp"

[api.headers]
Authorization = "Bearer abc123"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let api = parsed.mcp.iter().find(|s| s.name == "api").unwrap();
    assert_eq!(api.auth, McpAuthConfig::None);
    assert_eq!(
        api.headers.get("Authorization").map(String::as_str),
        Some("Bearer abc123")
    );
}

#[test]
fn mcp_auth_non_table_auth_field_defaults_to_none() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "http"
url = "http://example.com/mcp"
auth = "bearer"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let x = parsed.mcp.iter().find(|s| s.name == "x").unwrap();
    assert_eq!(x.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_missing_type_defaults_to_none() {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
token = "abc"
"#,
    )
    .unwrap();

    let parsed = McpConfig::from_toml(&mcp_value).expect("should parse");
    let x = parsed.mcp.iter().find(|s| s.name == "x").unwrap();
    assert_eq!(x.auth, McpAuthConfig::None);
}

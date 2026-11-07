use super::{McpAuthConfig, McpConfig};

#[test]
fn mcp_config_from_toml_table() {
    // -- Setup & Fixtures
    fn verify_map_shape(parsed: &McpConfig) {
        let c5t = parsed
            .mcp
            .iter()
            .find(|s| s.name == "c5t")
            .expect("c5t server exists");
        assert_eq!(c5t.url.as_deref(), Some("http://0.0.0.0:3737/mcp"));
        assert!(c5t.enabled);

        let nu_server = parsed
            .mcp
            .iter()
            .find(|s| s.name == "nu-server")
            .expect("nu-server fixture exists");
        assert_eq!(nu_server.command.as_deref(), Some("nu-mcp"));
        assert_eq!(nu_server.cwd.as_deref(), Some("/tmp"));
        assert_eq!(
            nu_server.args,
            vec!["--add-path".to_string(), "/tmp".to_string()]
        );
        assert_eq!(nu_server.env.get("GIT_PAGER").map(String::as_str), Some(""));
        assert!(!nu_server.enabled);
    }

    let map_shape: toml::Value = toml::from_str(
        r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
enabled = true

[nu-server]
transport = "stdio"
command = "nu-mcp"
cwd = "/tmp"
args = ["--add-path", "/tmp"]
enabled = false

[nu-server.env]
GIT_PAGER = ""
"#,
    )
    .unwrap();

    type Case = (&'static str, toml::Value, usize, Option<fn(&McpConfig)>);

    let cases: Vec<Case> = vec![
        ("map shape", map_shape, 2, Some(verify_map_shape)),
        (
            "missing mcp returns empty",
            toml::Value::Table(toml::value::Table::new()),
            0,
            None,
        ),
        (
            "non-table returns empty",
            toml::Value::String("not a table".to_string()),
            0,
            None,
        ),
    ];

    // -- Exec & Check
    for (name, value, expected_len, verify) in cases {
        let parsed = McpConfig::from_toml(&value).expect("should parse mcp config");
        assert_eq!(
            parsed.mcp.len(),
            expected_len,
            "case '{name}': server count"
        );
        if let Some(verify) = verify {
            verify(&parsed);
        }
    }
}

#[test]
fn mcp_config_enabled_defaults_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str)> = vec![
        (
            "omitted",
            r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
        ),
        (
            "non-bool",
            r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
enabled = "true"
"#,
        ),
    ];

    // -- Exec & Check
    for (name, toml_src) in cases {
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let parsed = McpConfig::from_toml(&value).expect("should parse");
        let c5t = parsed
            .mcp
            .iter()
            .find(|s| s.name == "c5t")
            .expect("c5t server exists");
        assert!(c5t.enabled, "case '{name}': enabled must default to true");
    }
}

#[test]
fn mcp_config_validation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &[&str])> = vec![
        (
            "empty stdio cwd",
            r#"
[nu]
transport = "stdio"
command = "nu-mcp"
cwd = "   "
"#,
            &["requires non-empty 'cwd'", "Invalid MCP configuration"],
        ),
        (
            "stdio requires command",
            r#"
[nu]
transport = "stdio"
"#,
            &["requires non-empty 'command'", "Invalid MCP configuration"],
        ),
        (
            "remote requires url",
            r#"
[c5t]
transport = "sse"
"#,
            &["requires non-empty 'url'", "Invalid MCP configuration"],
        ),
        (
            "unsupported transport",
            r#"
[x]
transport = "unsupported"
"#,
            &["unsupported transport", "Invalid transport"],
        ),
        (
            "reserved delimiter in server name",
            r#"
[gh__prod]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
            &["reserved delimiter", "Invalid MCP configuration"],
        ),
    ];

    // -- Exec & Check
    for (name, toml_src, expected_any) in cases {
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let err = McpConfig::from_toml(&value).expect_err("should fail");
        let msg = err.to_string();
        assert!(
            expected_any.iter().any(|fragment| msg.contains(fragment)),
            "case '{name}': message must contain one of {expected_any:?}, got: {msg}"
        );
    }
}

// ── McpAuthConfig tests ──────────────────────────────────────────────────────

#[test]
fn mcp_auth_parse_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str, McpAuthConfig)> = vec![
        (
            "defaults to none when omitted",
            "c5t",
            r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
"#,
            McpAuthConfig::None,
        ),
        (
            "parses none explicitly",
            "c5t",
            r#"
[c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"

[c5t.auth]
type = "none"
"#,
            McpAuthConfig::None,
        ),
        (
            "parses bearer",
            "api",
            r#"
[api]
transport = "http"
url = "https://api.example.com/mcp"

[api.auth]
type = "bearer"
token = "abc123"
"#,
            McpAuthConfig::Bearer {
                token: "abc123".to_string(),
            },
        ),
        (
            "parses oauth with scope",
            "ctx",
            r#"
[ctx]
transport = "http"
url = "https://mcp.context7.com/mcp"

[ctx.auth]
type = "oauth"
scope = "profile email"
"#,
            McpAuthConfig::OAuth {
                client_id: None,
                client_secret: None,
                scope: Some("profile email".to_string()),
                redirect_uri: None,
            },
        ),
        (
            "parses oauth with client credentials",
            "gh",
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
            McpAuthConfig::OAuth {
                client_id: Some("Iv1.example".to_string()),
                client_secret: Some("secret123".to_string()),
                scope: Some("repo read:org".to_string()),
                redirect_uri: None,
            },
        ),
        (
            "unknown type defaults to none",
            "x",
            r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
type = "unknown"
"#,
            McpAuthConfig::None,
        ),
        (
            "non-table auth field defaults to none",
            "x",
            r#"
[x]
transport = "http"
url = "http://example.com/mcp"
auth = "bearer"
"#,
            McpAuthConfig::None,
        ),
        (
            "missing type defaults to none",
            "x",
            r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
token = "abc"
"#,
            McpAuthConfig::None,
        ),
    ];

    // -- Exec & Check
    for (name, server_name, toml_src, expected) in cases {
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let parsed = McpConfig::from_toml(&value).expect("should parse");
        let server = parsed
            .mcp
            .iter()
            .find(|s| s.name == server_name)
            .expect("server exists");
        assert_eq!(server.auth, expected, "case '{name}': auth");
    }
}

#[test]
fn mcp_auth_validation_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &[&str])> = vec![
        (
            "bearer with empty token",
            r#"
[x]
transport = "http"
url = "http://example.com/mcp"

[x.auth]
type = "bearer"
token = ""
"#,
            &["empty token", "Invalid MCP configuration"],
        ),
        (
            "oauth on stdio",
            r#"
[x]
transport = "stdio"
command = "my-server"
cwd = "/tmp"

[x.auth]
type = "oauth"
scope = "profile"
"#,
            &[
                "requires HTTP or SSE transport",
                "Invalid MCP configuration",
            ],
        ),
    ];

    // -- Exec & Check
    for (name, toml_src, expected_any) in cases {
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let err = McpConfig::from_toml(&value).expect_err("should fail");
        let msg = err.to_string();
        assert!(
            expected_any.iter().any(|fragment| msg.contains(fragment)),
            "case '{name}': message must contain one of {expected_any:?}, got: {msg}"
        );
    }
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

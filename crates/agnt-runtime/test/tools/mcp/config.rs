use super::{McpAuthConfig, McpConfig};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn mcp_config_from_toml_table() -> Result<()> {
    // -- Setup & Fixtures
    fn verify_map_shape(parsed: &McpConfig) -> Result<()> {
        let c5t = parsed
            .mcp
            .iter()
            .find(|s| s.name == "c5t")
            .ok_or("c5t server must exist")?;
        assert_eq!(c5t.url.as_deref(), Some("http://0.0.0.0:3737/mcp"));
        assert!(c5t.enabled);

        let nu_server = parsed
            .mcp
            .iter()
            .find(|s| s.name == "nu-server")
            .ok_or("nu-server fixture must exist")?;
        assert_eq!(nu_server.command.as_deref(), Some("nu-mcp"));
        assert_eq!(nu_server.cwd.as_deref(), Some("/tmp"));
        assert_eq!(
            nu_server.args,
            vec!["--add-path".to_string(), "/tmp".to_string()]
        );
        assert_eq!(nu_server.env.get("GIT_PAGER").map(String::as_str), Some(""));
        assert!(!nu_server.enabled);

        Ok(())
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
    )?;

    type Case = (
        &'static str,
        toml::Value,
        usize,
        Option<fn(&McpConfig) -> Result<()>>,
    );

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
        let parsed = McpConfig::from_toml(&value)?;
        assert_eq!(
            parsed.mcp.len(),
            expected_len,
            "case '{name}': server count"
        );
        if let Some(verify) = verify {
            verify(&parsed)?;
        }
    }

    Ok(())
}

#[test]
fn mcp_config_enabled_defaults_table() -> Result<()> {
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
        let value: toml::Value = toml::from_str(toml_src)?;
        let parsed = McpConfig::from_toml(&value)?;
        let c5t = parsed
            .mcp
            .iter()
            .find(|s| s.name == "c5t")
            .ok_or("c5t server must exist")?;
        assert!(c5t.enabled, "case '{name}': enabled must default to true");
    }

    Ok(())
}

#[test]
fn mcp_config_validation_table() -> Result<()> {
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
        let value: toml::Value = toml::from_str(toml_src)?;
        let err = McpConfig::from_toml(&value).expect_err("should fail");
        let msg = err.to_string();
        assert!(
            expected_any.iter().any(|fragment| msg.contains(fragment)),
            "case '{name}': message must contain one of {expected_any:?}, got: {msg}"
        );
    }

    Ok(())
}

// ── McpAuthConfig tests ──────────────────────────────────────────────────────

#[test]
fn mcp_auth_parse_table() -> Result<()> {
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
        let value: toml::Value = toml::from_str(toml_src)?;
        let parsed = McpConfig::from_toml(&value)?;
        let server = parsed
            .mcp
            .iter()
            .find(|s| s.name == server_name)
            .ok_or("server must exist")?;
        assert_eq!(server.auth, expected, "case '{name}': auth");
    }

    Ok(())
}

#[test]
fn mcp_auth_validation_table() -> Result<()> {
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
        let value: toml::Value = toml::from_str(toml_src)?;
        let err = McpConfig::from_toml(&value).expect_err("should fail");
        let msg = err.to_string();
        assert!(
            expected_any.iter().any(|fragment| msg.contains(fragment)),
            "case '{name}': message must contain one of {expected_any:?}, got: {msg}"
        );
    }

    Ok(())
}

#[test]
fn mcp_auth_backwards_compat_headers_authorization_still_works() -> Result<()> {
    let mcp_value: toml::Value = toml::from_str(
        r#"
[api]
transport = "http"
url = "https://api.example.com/mcp"

[api.headers]
Authorization = "Bearer abc123"
"#,
    )?;

    let parsed = McpConfig::from_toml(&mcp_value)?;
    let api = parsed
        .mcp
        .iter()
        .find(|s| s.name == "api")
        .ok_or("api server must exist")?;
    assert_eq!(api.auth, McpAuthConfig::None);
    assert_eq!(
        api.headers.get("Authorization").map(String::as_str),
        Some("Bearer abc123")
    );

    Ok(())
}

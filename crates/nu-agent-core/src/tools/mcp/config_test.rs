use super::{McpAuthConfig, McpConfig};
use nu_protocol::{Record, Value, record};

#[test]
fn mcp_config_from_plugin_config_reads_map_shape() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
                "enabled" => Value::test_bool(true),
            }),
            "nu" => Value::test_record(record! {
                "transport" => Value::test_string("stdio"),
                "command" => Value::test_string("nu-mcp"),
                "cwd" => Value::test_string("/tmp"),
                "args" => Value::test_list(vec![
                    Value::test_string("--add-path"),
                    Value::test_string("/tmp"),
                ]),
                "env" => Value::test_record(record! {
                    "GIT_PAGER" => Value::test_string(""),
                }),
                "enabled" => Value::test_bool(false),
            }),
        }),
        "model" => Value::test_string("github-copilot/anthropic/claude-sonnet-4-20250514"),
        "providers" => Value::test_record(Record::new()),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse mcp config");
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
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse mcp config");
    let c5t = parsed
        .mcp
        .iter()
        .find(|s| s.name == "c5t")
        .expect("c5t server exists");
    assert!(c5t.enabled);
}

#[test]
fn mcp_config_enabled_rejects_non_bool_values() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
                "enabled" => Value::test_string("true"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("'enabled' must be a bool") || msg.contains("Invalid field type"));
}

#[test]
fn mcp_config_validation_rejects_empty_stdio_cwd_when_set() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "nu" => Value::test_record(record! {
                "transport" => Value::test_string("stdio"),
                "command" => Value::test_string("nu-mcp"),
                "cwd" => Value::test_string("   "),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("requires non-empty 'cwd'") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_config_from_plugin_config_missing_mcp_returns_empty() {
    let plugin_config = Value::test_record(record! {
        "model" => Value::test_string("openai/gpt-4o"),
        "providers" => Value::test_record(Record::new()),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("missing mcp should be ok");
    assert!(parsed.mcp.is_empty());
}

#[test]
fn mcp_config_from_plugin_config_rejects_non_record_mcp() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_list(vec![]),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("'mcp' must be a record") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_config_validation_requires_command_for_stdio() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "nu" => Value::test_record(record! {
                "transport" => Value::test_string("stdio"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("requires non-empty 'command'") || msg.contains("Invalid MCP configuration")
    );
}

#[test]
fn mcp_config_validation_requires_url_for_remote_transports() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("requires non-empty 'url'") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_config_rejects_unsupported_transport() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("unsupported"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("unsupported transport") || msg.contains("Invalid transport"));
}

#[test]
fn mcp_config_rejects_server_name_with_reserved_delimiter() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "gh__prod" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("reserved delimiter") || msg.contains("Invalid MCP configuration"));
}

// ── McpAuthConfig tests ──────────────────────────────────────────────────────

#[test]
fn mcp_auth_defaults_to_none_when_omitted() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
    let c5t = parsed.mcp.iter().find(|s| s.name == "c5t").unwrap();
    assert_eq!(c5t.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_parses_none_explicitly() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("none"),
                }),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
    let c5t = parsed.mcp.iter().find(|s| s.name == "c5t").unwrap();
    assert_eq!(c5t.auth, McpAuthConfig::None);
}

#[test]
fn mcp_auth_parses_bearer() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "api" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("https://api.example.com/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("bearer"),
                    "token" => Value::test_string("abc123"),
                }),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
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
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "ctx" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("https://mcp.context7.com/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("oauth"),
                    "scope" => Value::test_string("profile email"),
                }),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
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
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "gh" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("https://api.github.com/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("oauth"),
                    "client-id" => Value::test_string("Iv1.example"),
                    "client-secret" => Value::test_string("secret123"),
                    "scope" => Value::test_string("repo read:org"),
                }),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
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
fn mcp_auth_rejects_unknown_auth_type() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("http://example.com/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("unknown"),
                }),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("Unknown auth type") || msg.contains("Invalid auth type"));
}

#[test]
fn mcp_auth_rejects_bearer_with_empty_token() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("http://example.com/mcp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("bearer"),
                    "token" => Value::test_string(""),
                }),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("empty token") || msg.contains("Invalid MCP configuration"));
}

#[test]
fn mcp_auth_rejects_oauth_on_stdio() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("stdio"),
                "command" => Value::test_string("my-server"),
                "cwd" => Value::test_string("/tmp"),
                "auth" => Value::test_record(record! {
                    "type" => Value::test_string("oauth"),
                    "scope" => Value::test_string("profile"),
                }),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("requires HTTP or SSE transport") || msg.contains("Invalid MCP configuration")
    );
}

#[test]
fn mcp_auth_backwards_compat_headers_authorization_still_works() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "api" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("https://api.example.com/mcp"),
                "headers" => Value::test_record(record! {
                    "Authorization" => Value::test_string("Bearer abc123"),
                }),
            }),
        }),
    });

    let parsed = McpConfig::from_plugin_config(&plugin_config).expect("should parse");
    let api = parsed.mcp.iter().find(|s| s.name == "api").unwrap();
    assert_eq!(api.auth, McpAuthConfig::None);
    assert_eq!(
        api.headers.get("Authorization").map(String::as_str),
        Some("Bearer abc123")
    );
}

#[test]
fn mcp_auth_rejects_non_record_auth_field() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("http://example.com/mcp"),
                "auth" => Value::test_string("bearer"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("must be a record") || msg.contains("Invalid auth configuration"));
}

#[test]
fn mcp_auth_rejects_missing_type_field() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "x" => Value::test_record(record! {
                "transport" => Value::test_string("http"),
                "url" => Value::test_string("http://example.com/mcp"),
                "auth" => Value::test_record(record! {
                    "token" => Value::test_string("abc"),
                }),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("Missing 'type'") || msg.contains("Missing required field"));
}

use super::McpConfig;
use nu_protocol::{Record, Value, record};

#[test]
fn mcp_config_from_plugin_config_reads_map_shape() {
    let plugin_config = Value::test_record(record! {
        "mcp" => Value::test_record(record! {
            "c5t" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
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
            }),
        }),
        "model" => Value::test_string("github-copilot/anthropic/claude-sonnet-4.5"),
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

    let nu = parsed
        .mcp
        .iter()
        .find(|s| s.name == "nu")
        .expect("nu server exists");
    assert_eq!(nu.command.as_deref(), Some("nu-mcp"));
    assert_eq!(nu.cwd.as_deref(), Some("/tmp"));
    assert_eq!(nu.args, vec!["--add-path".to_string(), "/tmp".to_string()]);
    assert_eq!(nu.env.get("GIT_PAGER").map(String::as_str), Some(""));
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
            "gh::prod" => Value::test_record(record! {
                "transport" => Value::test_string("sse"),
                "url" => Value::test_string("http://0.0.0.0:3737/mcp"),
            }),
        }),
    });

    let err = McpConfig::from_plugin_config(&plugin_config).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("reserved delimiter") || msg.contains("Invalid MCP configuration"));
}

use super::*;

#[test]
fn test_validate_accepts_valid_configs() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Config)> = vec![
        (
            "full config",
            Config {
                api_key: Some("test-key".to_string()),
                base_url: Some("https://api.com".to_string()),
                temperature: Some(0.7),
                max_tokens: Some(1000),
                max_context_tokens: Some(4096),
                max_output_tokens: Some(2048),
                max_tool_turns: Some(20),
                ..base_config()
            },
        ),
        (
            "minimal config",
            Config {
                provider: "anthropic".to_string(),
                model: "claude-3-opus".to_string(),
                max_tool_turns: Some(20),
                ..Config::default()
            },
        ),
        (
            "max_output_tokens equals max_context_tokens",
            Config {
                max_context_tokens: Some(4000),
                max_output_tokens: Some(4000),
                ..base_config()
            },
        ),
    ];

    // -- Exec & Check
    for (name, config) in cases {
        assert!(config.validate().is_ok(), "{name} must validate");
    }
}

#[test]
fn test_validate_rejects_invalid_configs() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Config, &[&str])> = vec![
        (
            "empty provider",
            Config {
                provider: String::new(),
                ..base_config()
            },
            &["provider"],
        ),
        (
            "empty model",
            Config {
                model: String::new(),
                ..base_config()
            },
            &["model"],
        ),
        (
            "max_output_tokens exceeds max_context_tokens",
            Config {
                max_context_tokens: Some(2000),
                max_output_tokens: Some(3000),
                ..base_config()
            },
            &["max_output_tokens", "max_context_tokens"],
        ),
        (
            "zero max_tool_turns",
            Config {
                max_tool_turns: Some(0),
                ..base_config()
            },
            &["max_tool_turns"],
        ),
    ];

    // -- Exec & Check
    for (name, config, needles) in cases {
        let err = match config.validate() {
            Ok(_) => return Err(format!("{name} should fail validation").into()),
            Err(e) => e,
        };
        for needle in needles {
            assert!(
                err.contains(needle),
                "{name}: error must mention {needle}, got {err}"
            );
        }
    }
    Ok(())
}

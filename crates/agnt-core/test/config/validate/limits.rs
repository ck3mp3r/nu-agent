use super::*;

#[test]
fn test_validate_context_warning_threshold_bounds() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, f32, bool)> = vec![
        ("zero is rejected", 0.0, false),
        ("above one is rejected", 1.1, false),
        ("one is the inclusive upper bound", 1.0, true),
        ("typical value is accepted", 0.6, true),
    ];

    // -- Exec & Check
    for (name, threshold, expect_ok) in cases {
        let config = Config {
            context_warning_threshold: Some(threshold),
            ..base_config()
        };
        let result = config.validate();
        if expect_ok {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            let err = match result {
                Ok(_) => return Err(format!("{name} should fail validation").into()),
                Err(e) => e,
            };
            assert!(
                err.contains("context_warning_threshold"),
                "{name}: got {err}"
            );
        }
    }
    Ok(())
}

#[test]
fn test_validate_token_and_turn_limits() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Config, Option<&str>)> = vec![
        (
            "only max_context_tokens set",
            Config {
                max_context_tokens: Some(4096),
                ..base_config()
            },
            None,
        ),
        (
            "only max_output_tokens set",
            Config {
                max_output_tokens: Some(2048),
                ..base_config()
            },
            None,
        ),
        (
            "zero model_context_tokens",
            Config {
                model_context_tokens: Some(0),
                ..base_config()
            },
            Some("model_context_tokens"),
        ),
        (
            "one model_context_tokens",
            Config {
                model_context_tokens: Some(1),
                ..base_config()
            },
            None,
        ),
        (
            "no max_tool_turns",
            Config {
                max_tool_turns: None,
                ..base_config()
            },
            None,
        ),
        (
            "zero max_tool_turns",
            Config {
                max_tool_turns: Some(0),
                ..base_config()
            },
            Some("max_tool_turns"),
        ),
    ];

    // -- Exec & Check
    for (name, config, needle) in cases {
        let result = config.validate();
        match needle {
            None => assert!(result.is_ok(), "{name}: {result:?}"),
            Some(needle) => {
                let err = match result {
                    Ok(_) => return Err(format!("{name} should fail validation").into()),
                    Err(e) => e,
                };
                assert!(err.contains(needle), "{name}: got {err}");
            }
        }
    }
    Ok(())
}

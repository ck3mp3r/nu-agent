use super::*;

#[test]
fn test_validate_output_budget_rejects_invalid_values() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Config, &str)> = vec![
        (
            "multiplier <= 1.0",
            Config {
                output_budget_raise_multiplier: Some(1.0),
                ..base_config()
            },
            "output_budget_raise_multiplier",
        ),
        (
            "cap 0",
            Config {
                output_budget_raise_cap: Some(0),
                ..base_config()
            },
            "output_budget_raise_cap",
        ),
        (
            "unknown remedy mode",
            Config {
                output_budget_remedy_mode: Some("bogus".to_string()),
                ..base_config()
            },
            "output_budget_remedy_mode",
        ),
    ];

    // -- Exec & Check
    for (name, config, needle) in cases {
        let err = match config.validate() {
            Ok(_) => return Err(format!("{name} should fail validation").into()),
            Err(e) => e,
        };
        assert!(err.contains(needle), "{name}: got {err}");
    }
    Ok(())
}

#[test]
fn test_validate_output_budget_accepts_valid_remedy_modes() -> Result<()> {
    // -- Setup & Fixtures
    let modes = ["empty_output", "shorter_response"];

    // -- Exec & Check
    for mode in modes {
        let config = Config {
            output_budget_remedy_mode: Some(mode.to_string()),
            ..base_config()
        };
        assert!(config.validate().is_ok(), "mode {mode} must validate");
    }
    Ok(())
}

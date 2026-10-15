//! Persona config application tests for `runtime_build`.

use super::*;

#[test]
fn apply_persona_config_max_tokens_populates_config() {
    let mut config = Config::default();
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: None,
        max_tokens: Some(1024),
        max_tool_turns: None,
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false);

    assert_eq!(config.max_tokens, Some(1024));
}

#[test]
fn apply_persona_config_all_fields_when_config_empty() {
    let mut config = Config::default();
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.7),
        max_tokens: Some(2048),
        max_tool_turns: Some(10),
        max_tool_calls_per_subturn: Some(3),
        max_tool_result_bytes: Some(5000),
        additional_params: Some(serde_json::json!({"thinking": "disabled"})),
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false);

    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_tokens, Some(2048));
    assert_eq!(config.max_tool_turns, Some(10));
    assert_eq!(config.max_tool_calls_per_subturn, Some(3));
    assert_eq!(config.max_tool_result_bytes, Some(5000));
    assert!(config.additional_params.is_some());
}

#[test]
fn apply_persona_config_cli_wins_over_persona() -> Result<()> {
    let mut config = Config {
        temperature: Some(0.9),
        max_tokens: Some(4096),
        max_tool_calls_per_subturn: Some(8),
        max_tool_result_bytes: Some(20000),
        additional_params: Some(serde_json::json!({"from": "cli"})),
        ..Config::default()
    };
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.1),
        max_tokens: Some(512),
        max_tool_turns: None,
        max_tool_calls_per_subturn: Some(1),
        max_tool_result_bytes: Some(100),
        additional_params: Some(serde_json::json!({"from": "persona"})),
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false);

    // CLI values must survive
    assert_eq!(config.temperature, Some(0.9));
    assert_eq!(config.max_tokens, Some(4096));
    assert_eq!(config.max_tool_calls_per_subturn, Some(8));
    assert_eq!(config.max_tool_result_bytes, Some(20000));
    assert_eq!(
        config
            .additional_params
            .as_ref()
            .ok_or("should have additional_params set")?["from"],
        "cli"
    );
    Ok(())
}

#[test]
fn apply_persona_config_max_turns_cli_flag_wins() {
    let mut config = Config {
        max_tool_turns: Some(20),
        ..Config::default()
    };
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: None,
        max_tokens: None,
        max_tool_turns: Some(5),
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, true); // cli_max_turns_provided = true

    assert_eq!(config.max_tool_turns, Some(20)); // CLI wins
}

#[test]
fn apply_persona_config_max_turns_persona_overrides_default() {
    let mut config = Config {
        max_tool_turns: Some(20), // pipeline default
        ..Config::default()
    };
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: None,
        max_tokens: None,
        max_tool_turns: Some(5),
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false); // not a CLI flag

    assert_eq!(config.max_tool_turns, Some(5)); // persona wins over default
}

#[test]
fn apply_persona_config_partial_persona_leaves_others_unchanged() {
    let mut config = Config {
        max_tool_turns: Some(10),
        max_tool_result_bytes: Some(1000),
        ..Config::default()
    };
    let persona = agnt_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.7),
        max_tokens: None,
        max_tool_turns: None,
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, true);

    assert_eq!(config.temperature, Some(0.7)); // applied
    assert_eq!(config.max_tool_turns, Some(10)); // unchanged (cli flag)
    assert_eq!(config.max_tool_result_bytes, Some(1000)); // unchanged (persona had None)
}

//! CLI flag application tests for `runtime_build`.

use super::*;

// ── apply_cli_flags max_output_tokens tests ────────────────────────────────

#[test]
fn apply_cli_flags_max_output_tokens_sets_config() {
    let mut config = Config::default();
    let call = call_with_flags(vec![("max-output-tokens", Some(Value::test_int(4096)))]);

    super::apply_cli_flags(&mut config, &call);

    assert_eq!(config.max_output_tokens, Some(4096));
}

#[test]
fn apply_cli_flags_max_output_tokens_absent_leaves_none() {
    let mut config = Config::default();
    let call = call_with_flags(vec![]);

    super::apply_cli_flags(&mut config, &call);

    assert_eq!(config.max_output_tokens, None);
}

#[test]
fn apply_cli_flags_repetition_guard_off_sets_false() {
    let mut config = Config::default();
    let call = call_with_flags(vec![("repetition-guard", Some(Value::test_string("off")))]);

    super::apply_cli_flags(&mut config, &call);

    assert_eq!(config.repetition_guard, Some(false));
}

#[test]
fn apply_cli_flags_repetition_guard_on_sets_true() {
    let mut config = Config::default();
    let call = call_with_flags(vec![("repetition-guard", Some(Value::test_string("on")))]);

    super::apply_cli_flags(&mut config, &call);

    assert_eq!(config.repetition_guard, Some(true));
}

#[test]
fn apply_cli_flags_repetition_guard_absent_leaves_none() {
    let mut config = Config::default();
    let call = call_with_flags(vec![]);

    super::apply_cli_flags(&mut config, &call);

    assert_eq!(config.repetition_guard, None);
}

use super::*;

#[test]
fn nu_nonzero_exit_code_is_failure() {
    let result = resolve_success(
        "nu",
        true,
        r#"{"stdout":"","stderr":"error","exit_code":1}"#,
    );
    assert!(!result);
}

#[test]
fn nu_zero_exit_code_is_success() {
    let result = resolve_success("nu", true, r#"{"stdout":"ok","stderr":"","exit_code":0}"#);
    assert!(result);
}

#[test]
fn nu_parse_failure_falls_back_to_success() {
    let result = resolve_success("nu", true, "not json");
    assert!(result);
}

#[test]
fn other_tools_unaffected() {
    let result = resolve_success("read_file", true, r#"{"exit_code":1}"#);
    assert!(result);
}

#[test]
fn nu_base_failure_stays_failure() {
    let result = resolve_success("nu", false, r#"{"stdout":"","stderr":"","exit_code":0}"#);
    assert!(!result);
}

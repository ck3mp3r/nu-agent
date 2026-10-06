use nu_agent_core::transcript::ir::StyleHint;

use super::style_text;

#[test]
fn success_with_color_has_green_escape() {
    assert!(style_text("ok", &StyleHint::Success, true).contains("\x1b[32m"));
}

#[test]
fn no_color_returns_plain_text() {
    assert_eq!(style_text("plain", &StyleHint::Success, false), "plain");
}

#[test]
fn error_has_red_escape() {
    assert!(style_text("bad", &StyleHint::Error, true).contains("\x1b[31m"));
}

#[test]
fn muted_has_dim_escape() {
    assert!(style_text("dim", &StyleHint::Muted, true).contains("\x1b[2m"));
}

#[test]
fn normal_hint_has_no_escape() {
    assert_eq!(style_text("x", &StyleHint::Normal, true), "x");
}

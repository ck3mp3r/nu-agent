use super::*;
use rig::agent::ToolCallAction;

#[test]
fn under_limit_returns_none() {
    let cap = SubTurnCap::new(3);
    assert!(cap.check_and_increment("tool_a").is_none());
    assert!(cap.check_and_increment("tool_a").is_none());
    assert!(cap.check_and_increment("tool_a").is_none());
}

#[test]
fn at_limit_returns_skip() {
    let cap = SubTurnCap::new(2);
    cap.check_and_increment("tool_a");
    cap.check_and_increment("tool_a");
    let result = cap.check_and_increment("tool_a");
    assert!(matches!(result, Some(ToolCallAction::Skip { .. })));
}

#[test]
fn zero_limit_is_unlimited() {
    let cap = SubTurnCap::new(0);
    for _ in 0..50 {
        assert!(cap.check_and_increment("tool_a").is_none());
    }
}

#[test]
fn reset_clears_counter() {
    let cap = SubTurnCap::new(2);
    cap.check_and_increment("tool_a");
    cap.check_and_increment("tool_a");
    assert!(cap.check_and_increment("tool_a").is_some());
    cap.reset();
    assert!(cap.check_and_increment("tool_a").is_none());
}

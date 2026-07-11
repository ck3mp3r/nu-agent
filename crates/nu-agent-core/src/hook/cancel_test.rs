use super::*;
use rig::agent::Flow;
use tokio_util::sync::CancellationToken;

#[test]
fn check_hook_not_cancelled_returns_none() {
    let checker = CancelChecker {
        token: CancellationToken::new(),
    };
    assert!(checker.check_hook().is_none());
}

#[test]
fn check_hook_cancelled_returns_terminate() {
    let token = CancellationToken::new();
    token.cancel();
    let checker = CancelChecker { token };
    assert!(matches!(checker.check_hook(), Some(Flow::Terminate { .. })));
}

#[test]
fn check_tool_call_not_cancelled_returns_none() {
    let checker = CancelChecker {
        token: CancellationToken::new(),
    };
    assert!(checker.check_tool_call().is_none());
}

#[test]
fn check_tool_call_cancelled_returns_terminate() {
    let token = CancellationToken::new();
    token.cancel();
    let checker = CancelChecker { token };
    assert!(matches!(
        checker.check_tool_call(),
        Some(Flow::Terminate { .. })
    ));
}

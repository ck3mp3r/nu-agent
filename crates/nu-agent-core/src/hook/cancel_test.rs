use super::*;
use tokio_util::sync::CancellationToken;

#[test]
fn is_cancelled_returns_false_when_not_cancelled() {
    let checker = CancelChecker {
        token: CancellationToken::new(),
    };
    assert!(!checker.is_cancelled());
}

#[test]
fn is_cancelled_returns_true_when_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    let checker = CancelChecker { token };
    assert!(checker.is_cancelled());
}

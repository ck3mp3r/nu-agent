use super::{PollOutcome, poll_pending};
use std::sync::mpsc;

#[test]
fn poll_pending_returns_ready_ok_when_result_available() {
    let (tx, rx) = mpsc::sync_channel::<Result<String, String>>(1);
    tx.send(Ok("success".to_string())).unwrap();
    let outcome = poll_pending(rx);
    assert!(matches!(outcome, PollOutcome::Ready(Ok(ref s)) if s == "success"));
}

#[test]
fn poll_pending_returns_ready_err_when_error_available() {
    let (tx, rx) = mpsc::sync_channel::<Result<String, String>>(1);
    tx.send(Err("fail".to_string())).unwrap();
    let outcome = poll_pending(rx);
    assert!(matches!(outcome, PollOutcome::Ready(Err(ref s)) if s == "fail"));
}

#[test]
fn poll_pending_returns_pending_when_channel_empty() {
    let (_tx, rx) = mpsc::sync_channel::<Result<String, String>>(1);
    let outcome = poll_pending(rx);
    assert!(matches!(outcome, PollOutcome::Pending(_)));
}

#[test]
fn poll_pending_returns_disconnected_when_sender_dropped() {
    let (tx, rx) = mpsc::sync_channel::<Result<String, String>>(1);
    drop(tx);
    let outcome = poll_pending(rx);
    assert!(matches!(outcome, PollOutcome::Disconnected));
}

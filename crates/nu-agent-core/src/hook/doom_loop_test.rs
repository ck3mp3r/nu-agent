use super::*;
use rig::agent::ToolCallAction;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn make_ui_tx() -> (
    mpsc::UnboundedSender<crate::protocol::event::UiEvent>,
    mpsc::UnboundedReceiver<crate::protocol::event::UiEvent>,
) {
    mpsc::unbounded_channel()
}

#[test]
fn no_doom_loop_under_threshold() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let (tx, _rx) = make_ui_tx();

    for _ in 0..(DOOM_LOOP_THRESHOLD - 1) {
        let result = detector.check_and_record("read_file", "{\"path\": \"same\"}", &tx);
        assert!(result.is_none());
    }
}

#[test]
fn doom_loop_fires_at_threshold() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let (tx, _rx) = make_ui_tx();

    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = detector.check_and_record("read_file", "{\"path\": \"same\"}", &tx);
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(result.is_none(), "call {i} should not trip doom loop");
        } else {
            assert!(
                matches!(result, Some(ToolCallAction::Skip { .. })),
                "call {i} should skip with detection message"
            );
        }
    }
}

#[test]
fn doom_loop_state_reset_clears_signatures() {
    let mut state = DoomLoopState::default();
    for _ in 0..4 {
        state.check_and_record("tool", "args");
    }
    assert_eq!(state.recent_signatures.len(), 4);
    state.reset();
    assert_eq!(state.recent_signatures.len(), 0);
}

#[test]
fn different_args_does_not_trip_doom_loop() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let (tx, _rx) = make_ui_tx();

    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = detector.check_and_record("read_file", &format!("{{\"path\": \"{i}\"}}"), &tx);
        assert!(
            result.is_none(),
            "call {i} should not trip doom loop (different args)"
        );
    }
}

#[test]
fn cross_turn_reset_prevents_false_positive() {
    let mut state = DoomLoopState::default();

    // Simulate 4 identical calls in a failed turn
    for _ in 0..4 {
        assert!(
            state
                .check_and_record("read_file", "{\"path\": \"same\"}")
                .is_none()
        );
    }
    assert_eq!(state.recent_signatures.len(), 4);

    // Turn resets at start of next turn
    state.reset();
    assert_eq!(state.recent_signatures.len(), 0);

    // Only 1 more identical call on the new turn — should NOT trip
    let result = state.check_and_record("read_file", "{\"path\": \"same\"}");
    assert!(
        result.is_none(),
        "single call after reset must not trip doom loop"
    );
}

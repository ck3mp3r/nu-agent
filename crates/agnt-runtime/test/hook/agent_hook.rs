use crate::hook::agent_hook::DoomLoopState;
use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopDetection};
use std::sync::{Arc, Mutex};

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// DoomLoopState — direct test (no HookContext needed)
// ---------------------------------------------------------------------------

/// When a turn fails (no reset called), signatures remain accumulated.
#[test]
fn doom_state_not_reset_on_failed_turn() {
    let shared = Arc::new(Mutex::new(DoomLoopState::default()));

    // Manually accumulate signatures (simulating tool calls from a failed turn)
    {
        let mut state = shared.lock().unwrap();
        for i in 0..4 {
            state.check_and_record("read_file", &format!("{{\"path\": \"same{i}\"}}"));
        }
    }

    // Simulate failed turn: do NOT call reset()
    // Verify signatures are still accumulated
    let state = shared.lock().unwrap();
    assert_eq!(
        state.recent_signatures.len(),
        4,
        "signatures should persist after failed turn (no reset)"
    );
}

/// When a turn fails (no reset called), the escalation counter persists
/// alongside the signatures; reset clears it so the next detection is a first
/// detection again.
#[test]
fn doom_escalation_counter_persists_on_failed_turn_and_clears_on_reset() -> TestResult<()> {
    // -- Setup & Fixtures
    let shared = Arc::new(Mutex::new(DoomLoopState::default()));

    // -- Exec & Check
    // Accumulate to the first detection (simulating tool calls from a failed turn)
    {
        let mut state = shared.lock().map_err(|_| "should lock")?;
        for _ in 0..DOOM_LOOP_THRESHOLD - 1 {
            state.check_and_record("read_file", "{\"path\": \"same\"}");
        }
        let first = state.check_and_record("read_file", "{\"path\": \"same\"}");
        assert!(
            matches!(first, Some(DoomLoopDetection::First(_))),
            "threshold+1 call should be the first detection"
        );
    }

    // Simulate failed turn: do NOT call reset(). The counter must persist, so
    // the next detection in the same turn attempt escalates to Backoff.
    let second = {
        let mut state = shared.lock().map_err(|_| "should lock")?;
        state.check_and_record("read_file", "{\"path\": \"same\"}")
    };
    assert!(
        matches!(second, Some(DoomLoopDetection::Backoff(_))),
        "detection after failed turn (no reset) should be Backoff — counter persists"
    );

    // Reset clears the counter: the next detection is a first detection again.
    shared.lock().map_err(|_| "should lock")?.reset();
    let after_reset = {
        let mut state = shared.lock().map_err(|_| "should lock")?;
        for _ in 0..DOOM_LOOP_THRESHOLD - 1 {
            state.check_and_record("read_file", "{\"path\": \"same\"}");
        }
        state.check_and_record("read_file", "{\"path\": \"same\"}")
    };
    assert!(
        matches!(after_reset, Some(DoomLoopDetection::First(_))),
        "detection after reset should be First again — counter cleared"
    );
    Ok(())
}

use crate::hook::agent_hook::DoomLoopState;
use std::sync::{Arc, Mutex};

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

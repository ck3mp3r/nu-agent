use super::*;
use crate::hook::agent_hook::DoomLoopState;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// is_tool_failure — pure function, no HookContext needed
// ---------------------------------------------------------------------------

#[test]
fn is_tool_failure_detects_all_failure_variants() {
    assert!(is_tool_failure("Toolset error: something went wrong"));
    assert!(is_tool_failure("Permission denied"));
    assert!(is_tool_failure("Doom loop detected: 'nu' called 3 times"));
    assert!(is_tool_failure("Tool 'nonexistent' is not available."));
}

#[test]
fn is_tool_failure_does_not_flag_success() {
    assert!(!is_tool_failure("ok"));
    assert!(!is_tool_failure(""));
    assert!(!is_tool_failure("ls output here"));
}

#[test]
fn is_tool_failure_detects_tool_call_limit() {
    assert!(is_tool_failure(
        "Sub-turn tool call limit reached (5). No further tools will be called in this response. \
         Please summarise what you have accomplished so far and continue in the next turn if needed."
    ));
}

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

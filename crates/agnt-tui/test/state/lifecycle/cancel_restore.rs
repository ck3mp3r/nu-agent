use super::*;

/// `cancel_and_restore_pending_to_input` drains pending texts into one string,
/// returns none when nothing is pending, and is a no-op on idle.
#[test]
fn cancel_and_restore_pending_to_input() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.enqueue_prompt("alpha".to_string());
    let _ = state.activate_next_prompt();
    state.enqueue_prompt("beta".to_string());
    state.enqueue_prompt("gamma".to_string());
    let interrupted_id = active_prompt_id(&state).ok_or("should have an active prompt")?;
    let pending_ids = pending_prompt_ids(&state);
    let status_of = |state: &AppState, id: u64| {
        state
            .prompt_items()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.status)
    };

    // -- Exec
    let result = state.cancel_and_restore_pending_to_input();

    // -- Check: pending texts are drained and the phase returns to idle.
    assert_eq!(result, Some("beta\n\ngamma".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(active_prompt_id(&state), None);
    assert_eq!(state.phase, UiPhase::Idle);

    // -- Check: the interrupted prompt is Done, the pendings are Cancelled.
    assert_eq!(status_of(&state, interrupted_id), Some(PromptStatus::Done));
    for pending_id in pending_ids {
        assert_eq!(status_of(&state, pending_id), Some(PromptStatus::Cancelled));
    }

    // -- Setup & Fixtures: no pending prompts.
    let mut none = AppState::default();
    none.enqueue_prompt("only".to_string());
    let _ = none.activate_next_prompt();

    // -- Exec & Check
    assert_eq!(none.cancel_and_restore_pending_to_input(), None);
    assert_eq!(none.phase, UiPhase::Idle);

    // -- Setup & Fixtures: idle.
    let mut idle = AppState::default();

    // -- Exec & Check
    assert_eq!(idle.cancel_and_restore_pending_to_input(), None);

    // -- Setup & Fixtures: coalesced dispatch.
    let mut state = AppState::default();
    state.enqueue_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.enqueue_prompt("third".to_string());
    state.phase = UiPhase::Idle;
    state.active_cycle = false;

    // -- Exec
    let result = state.take_next_prompt_for_execution();

    // -- Check
    assert_eq!(result, Some("first\nsecond\nthird".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(state.phase, UiPhase::Busy);

    // -- Setup & Fixtures: a single pending prompt.
    let mut single = AppState::default();
    single.enqueue_prompt("only".to_string());
    single.phase = UiPhase::Idle;
    single.active_cycle = false;

    // -- Exec & Check
    assert_eq!(
        single.take_next_prompt_for_execution(),
        Some("only".to_string())
    );
    assert!(pending_prompt_ids(&single).is_empty());

    // -- Setup & Fixtures: an empty queue.
    let mut empty = AppState::default();

    // -- Exec & Check
    assert_eq!(empty.take_next_prompt_for_execution(), None);
    Ok(())
}

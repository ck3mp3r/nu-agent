//! The turn-end release belongs to the strict spine.
//!
//! The stream ring is lossy, so its `TurnRecord::Completed` is display-only: a
//! lagged subscription that drops the record must not strand `input_locked`,
//! and a stale record that folds after the next turn started must not touch
//! the turn that now owns the surface.

use super::*;

use agnt_runtime::protocol::events::NoticeKind;

#[test]
fn test_spine_turn_completed_releases_locked_input() -> Result<()> {
    // -- Setup & Fixtures: a running turn the surface locked.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(&mut state, inference_started());
    assert!(state.input_locked, "the turn start locks the input");
    assert_eq!(state.phase, UiPhase::Busy);

    // -- Exec: the spine step folds; no ring record is delivered at all.
    let changed = fold_turn_completed(&mut state);

    // -- Check: the spine step alone releases the turn.
    assert!(changed, "the spine step changes the surface");
    assert!(
        !state.input_locked,
        "a lagged ring must not strand the input lock"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert_eq!(active_prompt_id(&state), None);
    assert_reducer_invariants(&state);

    // -- Exec & Check: the release sequence running twice for one turn leaves
    // the settled state alone.
    let settled = (
        state.phase,
        state.input_locked,
        state.is_active_cycle(),
        state.transcript.len(),
    );
    fold_turn_completed(&mut state);
    assert_eq!(
        (
            state.phase,
            state.input_locked,
            state.is_active_cycle(),
            state.transcript.len(),
        ),
        settled,
        "a repeated release stays consistent"
    );
    assert_eq!(active_prompt_id(&state), None);
    Ok(())
}

#[test]
fn test_stale_ring_completed_after_spine_release_is_noop() -> Result<()> {
    // -- Setup & Fixtures: a running turn with a prompt stacked behind it.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(&mut state, inference_started());
    state.enqueue_prompt("next".to_string());

    // -- Exec: the spine releases the turn and the loop drains the queue into
    // the next turn, which a prompt typed while busy then follows.
    fold_turn_completed(&mut state);
    let dispatched = state.take_next_prompt_for_execution();
    state.enqueue_prompt("later".to_string());

    let next_active = active_prompt_id(&state);
    let pending_before = pending_prompt_ids(&state);
    let transcript_before = state.transcript.entries().to_vec();
    let phase_before = state.phase;
    let locked_before = state.input_locked;

    // -- Check: the next turn owns the surface before the stale record folds.
    assert_eq!(dispatched.as_deref(), Some("next"));
    assert!(
        next_active.is_some(),
        "the drained prompt is the active turn"
    );
    assert_eq!(phase_before, UiPhase::Busy);

    // -- Exec: the ring's stale completion folds last.
    let changed = reduce_ui(&mut state, turn_completed(0));

    // -- Check: the stale record moves nothing.
    assert!(!changed, "a stale ring completion changes nothing");
    assert_eq!(state.input_locked, locked_before, "no lock state changes");
    assert_eq!(state.phase, phase_before, "no phase changes");
    assert_eq!(
        active_prompt_id(&state),
        next_active,
        "the next turn stays active"
    );
    assert_eq!(
        pending_prompt_ids(&state),
        pending_before,
        "no queue drains"
    );
    assert_eq!(state.transcript.entries(), transcript_before);
    Ok(())
}

/// A failed turn's release rides the spine, never the narration lane.
///
/// The `TurnError` notice is narration, and narration is the lossy lane: a
/// dropped notice must not strand the input lock. The failed turn's spine step
/// is the release, and it is the only one that has to arrive.
#[test]
fn test_failed_turn_spine_step_releases_with_narration_silent() -> Result<()> {
    // -- Setup & Fixtures: a running turn the surface locked.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(&mut state, inference_started());
    assert!(state.input_locked, "the turn start locks the input");
    assert_eq!(state.phase, UiPhase::Busy);

    // -- Exec: the failed turn's spine step folds; the `TurnError` notice never
    // arrives.
    let changed = fold_turn_completed(&mut state);

    // -- Check: the spine step alone releases the turn, so the composer accepts
    // a new prompt again.
    assert!(changed, "the spine step changes the surface");
    assert!(
        !state.input_locked,
        "a dropped error notice must not strand the input lock"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert_eq!(active_prompt_id(&state), None);
    assert_reducer_invariants(&state);

    // -- Exec: the notice arrives late, after the release.
    let before = state.transcript.len();
    reduce_narration(
        &mut state,
        system_message(NoticeKind::TurnError, "provider exploded"),
    );

    // -- Check: the error keeps its transcript row and moves no lock state.
    assert_eq!(
        state.transcript.len(),
        before + 1,
        "the late notice still renders its row"
    );
    assert!(
        !state.input_locked,
        "a late error notice must not re-lock the input"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert_reducer_invariants(&state);
    Ok(())
}

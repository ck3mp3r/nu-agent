//! Rows 7, 11, 12, 13, 16, 17: the submit lanes, the prompt queue, and the
//! stack drain.

use super::*;

// row 7 — Enter under the lock queues the draft instead of running it.
#[test]
fn test_input_matrix_locked_enter_queues_pending_prompt() -> Result<()> {
    // -- Setup & Fixtures: a running turn with a dispatched draft.
    let mut state = busy_state_with_clean_transcript();
    state.input.pending_submit_text = Some("typed while busy".to_string());

    // -- Exec
    let changed = fold_key(&mut state, Key::Enter, None);

    // -- Check: the text queues, the residue goes, the turn keeps the cycle.
    assert!(changed);
    assert_eq!(pending_prompt_ids(&state).len(), 1);
    assert_eq!(state.input.pending_submit_text, None);
    assert_eq!(
        state.prompt_items().last().map(|p| p.prompt_text.as_str()),
        Some("typed while busy")
    );
    assert!(active_prompt_id(&state).is_some());

    // -- Exec & Check: the drain waits for the running turn.
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    Ok(())
}

// row 11 — an empty Enter submits nothing.
#[test]
fn test_input_matrix_empty_enter_submits_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    assert_eq!(state.input.pending_submit_text, None);

    // -- Exec & Check
    assert!(!fold_key(&mut state, Key::Enter, None));
    assert_eq!(state.phase, UiPhase::Idle);
    assert_eq!(state.pending_prompt_count(), 0);
    assert!(state.transcript.entries().is_empty());
    Ok(())
}

// row 12 — the queue paints as stacked rows and drains exactly once.
#[test]
fn test_input_matrix_queue_drains_once_after_release() -> Result<()> {
    // -- Setup & Fixtures: a running turn with two prompts typed under the lock.
    let mut state = busy_state_with_clean_transcript();
    for text in ["first", "second"] {
        state.input.pending_submit_text = Some(text.to_string());
        fold_key(&mut state, Key::Enter, None);
    }
    assert_eq!(state.pending_prompt_count(), 2);

    // -- Check: one user row per queued prompt, in queue order.
    let stacked: Vec<String> = queued_prompt_entries(&state)
        .iter()
        .map(entry_text)
        .collect();
    assert_eq!(stacked, vec!["first".to_string(), "second".to_string()]);

    // -- Exec: the running turn's spine step releases the lock.
    fold_turn_completed(&mut state);

    // -- Check: one drain, one block, queue order kept, nothing restored. (The
    // interrupt gesture is pinned end-to-end by
    // `test/runtime/submit_cancel/cancel_paths.rs`.)
    assert_eq!(
        state.take_next_prompt_for_execution().as_deref(),
        Some("first\nsecond")
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.input.restored_input_text, None);
    Ok(())
}

// row 13 — the history keys do nothing under a running turn.
#[test]
fn test_input_matrix_history_keys_do_nothing_while_busy() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = busy_state_with_clean_transcript();
    let items_before = state.prompt_items().len();

    // -- Exec & Check: history is the coordinator's; neither fold moves state.
    assert!(!fold_key(&mut state, Key::Up, None));
    assert!(!fold_key(&mut state, Key::Down, None));
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.input.pending_submit_text, None);
    assert_eq!(state.prompt_items().len(), items_before);
    Ok(())
}

// row 16 — Enter with no draft never double-stacks the held queue.
#[test]
fn test_input_matrix_locked_enter_without_draft_never_double_stacks() -> Result<()> {
    // -- Setup & Fixtures: a running turn with one prompt queued under the lock.
    let mut state = busy_state_with_clean_transcript();
    state.input.pending_submit_text = Some("queued".to_string());
    fold_key(&mut state, Key::Enter, None);
    assert_eq!(state.pending_prompt_count(), 1);
    assert_eq!(state.input.pending_submit_text, None);
    let items_before = state.prompt_items().len();

    // -- Exec: the interrupt's key folds with nothing dispatched.
    let changed = fold_key(&mut state, Key::Enter, None);

    // -- Check
    assert!(!changed);
    assert_eq!(state.pending_prompt_count(), 1);
    assert_eq!(state.prompt_items().len(), items_before);
    assert_eq!(state.phase, UiPhase::Busy);
    Ok(())
}

// row 17 — normal-mode Enter refuses the submission.
#[test]
fn test_input_matrix_normal_enter_refuses_the_submission() -> Result<()> {
    // -- Setup & Fixtures: an idle surface in normal mode.
    let mut idle = AppState::default();
    idle.enter_normal_mode();

    // -- Exec & Check: nothing queues, nothing is consumed.
    assert!(!fold_key(&mut idle, Key::Enter, None));
    assert_eq!(idle.phase, UiPhase::Idle);
    assert_eq!(idle.pending_prompt_count(), 0);
    assert_eq!(idle.input.pending_submit_text, None);

    // -- Setup & Fixtures: the same key on a running turn in normal mode.
    let mut busy = busy_state_with_clean_transcript();
    busy.enter_normal_mode();

    // -- Exec & Check: the running turn's own prompt is the only one there is.
    assert!(!fold_key(&mut busy, Key::Enter, None));
    assert_eq!(busy.phase, UiPhase::Busy);
    assert_eq!(busy.pending_prompt_count(), 0);
    assert_eq!(busy.input.pending_submit_text, None);
    Ok(())
}

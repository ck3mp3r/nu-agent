use super::*;

/// History navigation loads the last submitted prompt, cycles newest first,
/// clamps at the oldest, restores the draft, and resets on demand.
#[test]
fn history_navigation_cycles_and_resets() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    for t in ["a", "b", "c"] {
        state.enqueue_prompt(t.to_string());
        let _ = state.activate_next_prompt();
        state.complete_active_prompt();
    }
    let submitted = state.submitted_prompt_texts();

    // -- Exec & Check: newest first, clamped at the oldest.
    assert_eq!(
        state.input.history_up(&submitted, ""),
        Some("c".to_string())
    );
    assert_eq!(
        state.input.history_up(&submitted, ""),
        Some("b".to_string())
    );
    assert_eq!(
        state.input.history_up(&submitted, ""),
        Some("a".to_string())
    );
    assert_eq!(
        state.input.history_up(&submitted, ""),
        Some("a".to_string())
    );

    // -- Exec & Check: history down past the newest restores the draft.
    let mut draft = AppState::default();
    draft.enqueue_prompt("p1".to_string());
    let _ = draft.activate_next_prompt();
    draft.complete_active_prompt();
    let submitted = draft.submitted_prompt_texts();
    let _ = draft.input.history_up(&submitted, "draft");
    assert_eq!(
        draft.input.history_down(&submitted),
        Some("draft".to_string())
    );

    // -- Exec & Check: history up returns the previous text for a multiline buffer.
    let mut multiline = AppState::default();
    multiline.enqueue_prompt("prev".to_string());
    let _ = multiline.activate_next_prompt();
    multiline.complete_active_prompt();
    let submitted = multiline.submitted_prompt_texts();
    assert_eq!(
        multiline.input.history_up(&submitted, "line1\nline2"),
        Some("prev".to_string())
    );
    assert_eq!(
        multiline.input.history_up(&submitted, "ab\nxyz"),
        Some("prev".to_string())
    );

    // -- Setup & Fixtures: an Esc-Esc stop with one active and one pending prompt.
    let mut interrupted = AppState::default();
    interrupted.enqueue_prompt("interrupted".to_string());
    let _ = interrupted.activate_next_prompt();
    interrupted.enqueue_prompt("pending".to_string());

    // -- Exec
    let restored = interrupted.cancel_and_restore_pending_to_input();
    let submitted = interrupted.submitted_prompt_texts();

    // -- Check: the interrupted prompt is recallable, the pending one is not.
    assert_eq!(restored, Some("pending".to_string()));
    assert_eq!(
        interrupted.input.history_up(&submitted, ""),
        Some("interrupted".to_string())
    );
    assert!(!submitted.contains(&"pending".to_string()));

    // -- Exec & Check: history down with no history returns none.
    let mut none = AppState::default();
    let submitted = none.submitted_prompt_texts();
    assert_eq!(none.input.history_down(&submitted), None);

    // -- Exec & Check: reset clears the navigation index.
    let mut reset = AppState::default();
    reset.enqueue_prompt("p1".to_string());
    let _ = reset.activate_next_prompt();
    reset.complete_active_prompt();
    let submitted = reset.submitted_prompt_texts();
    let _ = reset.input.history_up(&submitted, "");
    assert_eq!(
        reset.input.history_up(&submitted, ""),
        Some("p1".to_string())
    );
    reset.input.reset_history_navigation();
    assert_eq!(reset.input.history_down(&submitted), None);
    reset.input.reset_history_navigation();
    assert_eq!(reset.input.history_down(&submitted), None);

    // -- Setup & Fixtures: the insert-exit chord.
    let mut state = AppState::default();

    // -- Exec & Check: not pending by default, pending after arming.
    assert!(!state.input.insert_exit_pending_j());
    state.input.set_insert_exit_pending_j();
    assert!(state.input.insert_exit_pending_j());

    // -- Exec & Check: clears on demand.
    state.input.clear_insert_exit_pending_j();
    assert!(!state.input.insert_exit_pending_j());
    state.input.clear_insert_exit_pending_j();
    assert!(!state.input.insert_exit_pending_j());

    // -- Setup & Fixtures: a short expiry so the window elapses without a wait.
    let mut expiring = AppState {
        input: InputState::default().with_insert_exit_expiry(std::time::Duration::from_millis(10)),
        ..Default::default()
    };
    expiring.input.set_insert_exit_pending_j();

    // -- Exec
    std::thread::sleep(std::time::Duration::from_millis(30));

    // -- Check
    assert!(!expiring.input.insert_exit_pending_j());
    Ok(())
}

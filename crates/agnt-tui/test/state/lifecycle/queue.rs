use super::*;

/// Submitting a prompt keeps input editable while queuing; the abort
/// confirmation requires a busy context and finalize resets it.
#[test]
fn queueing_abort_and_finalize_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: enqueue moves to busy but keeps input editable.
    state.enqueue_prompt("check cluster status".to_string());
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Queued);

    // -- Exec & Check: activating the prompt marks it in progress.
    let _ = state.activate_next_prompt();
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    // -- Exec & Check: abort confirmation requires a busy context.
    assert!(state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert!(!state.input_locked);

    // -- Setup & Fixtures: idle state.
    let mut idle = AppState::default();

    // -- Exec & Check: abort confirmation is refused while idle.
    assert!(!idle.request_abort_confirmation());
    assert_eq!(idle.phase, UiPhase::Idle);

    // -- Setup & Fixtures: a busy state with a fresh confirmation marker.
    let mut finalizing = AppState::default();
    finalizing.enqueue_prompt("run".to_string());
    let _ = finalizing.activate_next_prompt();
    let marker = finalizing.abort.confirmation_marker;
    assert!(finalizing.request_abort_confirmation());
    assert!(finalizing.abort.confirmation_marker > marker);

    // -- Exec
    finalizing.finalize_cycle();

    // -- Check
    assert_eq!(finalizing.phase, UiPhase::Idle);
    assert!(!finalizing.input_locked);
    assert!(!finalizing.abort.pending);
    assert_eq!(finalizing.prompt_items()[0].status, PromptStatus::Done);

    // -- Setup & Fixtures: the FIFO queue.
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());

    // -- Exec & Check: FIFO activation.
    assert_eq!(pending_prompt_ids(&state), vec![1, 2, 3]);
    assert_eq!(state.activate_next_prompt(), Some(1));
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[1].status, PromptStatus::Queued);
    assert_eq!(state.prompt_items()[2].status, PromptStatus::Queued);
    state.complete_active_prompt();
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
    assert_eq!(state.activate_next_prompt(), Some(2));
    assert_eq!(active_prompt_id(&state), Some(2));
    state.complete_active_prompt();
    assert_eq!(state.activate_next_prompt(), Some(3));
    state.complete_active_prompt();
    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![PromptStatus::Done, PromptStatus::Done, PromptStatus::Done]
    );

    // -- Setup & Fixtures: a fresh queue with one active prompt.
    let mut aborting = AppState::default();
    aborting.enqueue_prompt("p1".to_string());
    aborting.enqueue_prompt("p2".to_string());
    aborting.enqueue_prompt("p3".to_string());
    let _ = aborting.activate_next_prompt();
    let interrupted_id = active_prompt_id(&aborting).ok_or("should have an active prompt")?;
    let pending_ids = pending_prompt_ids(&aborting);
    let status_of = |state: &AppState, id: u64| {
        state
            .prompt_items()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.status)
    };

    // -- Exec
    let restored = aborting.cancel_and_restore_pending_to_input();

    // -- Check: pending texts drain back to the input and the queue empties.
    assert_eq!(restored, Some("p2\n\np3".to_string()));
    assert_eq!(active_prompt_id(&aborting), None);
    assert!(pending_prompt_ids(&aborting).is_empty());

    // -- Check: the interrupted prompt is Done, the pendings are Cancelled.
    assert_eq!(
        status_of(&aborting, interrupted_id),
        Some(PromptStatus::Done)
    );
    for pending_id in pending_ids {
        assert_eq!(
            status_of(&aborting, pending_id),
            Some(PromptStatus::Cancelled)
        );
    }
    Ok(())
}

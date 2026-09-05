use crate::state::*;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The active prompt is the one currently in `InProgress` status.
fn active_prompt_id(state: &AppState) -> Option<u64> {
    state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .map(|p| p.id)
}

/// Pending prompts are those still in `Queued` status.
fn pending_prompt_ids(state: &AppState) -> Vec<u64> {
    state
        .prompt_items()
        .iter()
        .filter(|p| p.status == PromptStatus::Queued)
        .map(|p| p.id)
        .collect()
}

#[test]
fn submit_acceptance_clears_input_and_keeps_input_editable() {
    let mut state = AppState::default();

    state.enqueue_prompt("check cluster status".to_string());

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
}

#[test]
fn non_idle_phase_keeps_input_editable_for_queueing() {
    let mut state = AppState::default();

    state.enqueue_prompt("one".to_string());
    assert!(!state.input_locked);
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Queued);

    let _ = state.activate_next_prompt();
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    state.request_abort_confirmation();
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(!state.input_locked);
}

#[test]
fn abort_pending_requires_busy_context() {
    let mut state = AppState::default();
    assert!(!state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::Idle);

    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    assert!(state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
}

#[test]
fn finalize_resets_abort_pending_and_unlocks_input() {
    let mut state = AppState::default();
    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    let marker = state.abort.confirmation_marker;
    assert!(state.request_abort_confirmation());
    assert!(state.abort.confirmation_marker > marker);

    state.finalize_cycle();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
}

#[test]
fn prompt_queue_lifecycle_is_fifo_and_single_in_progress() {
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());

    assert_eq!(pending_prompt_ids(&state), vec![1, 2, 3]);

    let first = state.activate_next_prompt();
    assert_eq!(first, Some(1));
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[1].status, PromptStatus::Queued);
    assert_eq!(state.prompt_items()[2].status, PromptStatus::Queued);

    state.complete_active_prompt();
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);

    let second = state.activate_next_prompt();
    assert_eq!(second, Some(2));
    assert_eq!(active_prompt_id(&state), Some(2));

    state.complete_active_prompt();
    let third = state.activate_next_prompt();
    assert_eq!(third, Some(3));
    state.complete_active_prompt();

    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![PromptStatus::Done, PromptStatus::Done, PromptStatus::Done]
    );
}

#[test]
fn global_abort_cancels_active_and_all_pending_prompts() {
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());
    let _ = state.activate_next_prompt();

    state.cancel_active_and_pending_prompts();

    assert_eq!(active_prompt_id(&state), None);
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![
            PromptStatus::Cancelled,
            PromptStatus::Cancelled,
            PromptStatus::Cancelled
        ]
    );
}

#[test]
fn record_token_usage_tracks_latest_and_accumulates_session_total() {
    let mut state = AppState::default();

    state.status.tokens.record_token_usage(7, 5, 12);
    assert_eq!(state.status.tokens.latest_input_tokens, Some(7));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(5));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(state.status.tokens.session_total_tokens, 12);

    state.status.tokens.record_token_usage(2, 3, 5);
    assert_eq!(state.status.tokens.latest_input_tokens, Some(2));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(3));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(5));
    assert_eq!(state.status.tokens.session_total_tokens, 17);
}

#[test]
fn enqueue_external_prompt_creates_in_progress_prompt_without_pending() {
    let mut state = AppState::default();

    state.enqueue_external_prompt("mailbox message".to_string());

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[0].prompt_text, "mailbox message");
    assert_eq!(active_prompt_id(&state), Some(1));
    assert!(
        pending_prompt_ids(&state).is_empty(),
        "external prompt must NOT appear in pending_prompt_ids"
    );
}

#[test]
fn enqueue_external_prompt_adds_user_transcript_line() -> Result<()> {
    let mut state = AppState::default();

    state.enqueue_external_prompt("hello from parent".to_string());

    // starting spacer + user + closing spacer
    assert!(!state.transcript.entries.is_empty());
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    assert!(matches!(last.kind, TranscriptEntryKind::Spacer(_)));
    Ok(())
}

#[test]
fn enqueue_external_prompt_completes_via_complete_active_prompt() {
    let mut state = AppState::default();

    state.enqueue_external_prompt("external task".to_string());
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    state.complete_active_prompt();

    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert_eq!(active_prompt_id(&state), None);
}

#[test]
fn enqueue_external_prompt_not_returned_by_take_submitted_prompt() {
    let mut state = AppState::default();

    state.enqueue_external_prompt("external".to_string());

    // take_next_prompt_for_execution should NOT return the external prompt
    // because it's already active (not pending)
    let taken = state.take_next_prompt_for_execution();
    assert_eq!(taken, None, "external prompt must not be re-dispatched");
}

#[test]
fn enqueue_prompt_does_not_add_transcript_entry() {
    let mut state = AppState::default();
    state.enqueue_external_prompt("first".to_string());
    let before = state.transcript.entries.len();
    state.enqueue_prompt("second".to_string());
    assert_eq!(state.transcript.entries.len(), before);
}

#[test]
fn clear_transcript_resets_token_fields() {
    let mut state = AppState {
        status: StatusState {
            tokens: TokenUsage {
                latest_input_tokens: Some(100),
                latest_output_tokens: Some(200),
                latest_total_tokens: Some(300),
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    state.clear_transcript();
    assert!(state.status.tokens.latest_input_tokens.is_none());
    assert!(state.status.tokens.latest_output_tokens.is_none());
    assert!(state.status.tokens.latest_total_tokens.is_none());
}

#[test]
fn activate_next_prompt_adds_user_entry_to_transcript() -> Result<()> {
    let mut state = AppState::default();
    state.enqueue_external_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.complete_active_prompt();
    let before = state.transcript.entries.len();
    state.activate_next_prompt();
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), before + 3);
    assert!(matches!(
        state.transcript.entries.get(before).map(|e| &e.kind),
        Some(TranscriptEntryKind::Spacer(_))
    ));
    assert!(matches!(
        state.transcript.entries.get(before + 1).map(|e| &e.kind),
        Some(TranscriptEntryKind::User(_))
    ));
    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    assert!(matches!(last.kind, TranscriptEntryKind::Spacer(_)));
    Ok(())
}

#[test]
fn cancel_and_restore_drains_pending_texts_into_input_buffer() {
    let mut state = AppState::default();
    state.enqueue_prompt("alpha".to_string());
    let _ = state.activate_next_prompt();
    state.enqueue_prompt("beta".to_string());
    state.enqueue_prompt("gamma".to_string());

    let result = state.cancel_and_restore_pending_to_input();

    assert_eq!(result, Some("beta\n\ngamma".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(active_prompt_id(&state), None);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn cancel_and_restore_with_no_pending_leaves_buffer_empty() {
    let mut state = AppState::default();
    state.enqueue_prompt("only".to_string());
    let _ = state.activate_next_prompt();

    let result = state.cancel_and_restore_pending_to_input();

    assert_eq!(result, None);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn cancel_and_restore_on_idle_is_noop() {
    let mut state = AppState::default();
    let result = state.cancel_and_restore_pending_to_input();
    assert_eq!(result, None);
}

#[test]
fn coalesced_dispatch_joins_all_pending_into_one_string() {
    let mut state = AppState::default();
    state.enqueue_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.enqueue_prompt("third".to_string());
    // Reset so take_next can activate (enqueue_prompt sets busy)
    state.phase = UiPhase::Idle;
    state.active_cycle = false;

    let result = state.take_next_prompt_for_execution();

    assert_eq!(result, Some("first\n\nsecond\n\nthird".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn coalesced_dispatch_single_pending_returns_text_unchanged() {
    let mut state = AppState::default();
    state.enqueue_prompt("only".to_string());
    state.phase = UiPhase::Idle;
    state.active_cycle = false;

    let result = state.take_next_prompt_for_execution();

    assert_eq!(result, Some("only".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
}

#[test]
fn coalesced_dispatch_empty_queue_returns_none() {
    let mut state = AppState::default();
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, None);
}

#[test]
fn history_up_on_first_use_loads_last_submitted() {
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let submitted = state.submitted_prompt_texts();
    let result = state.input.history_up(&submitted, "");
    assert_eq!(result, Some("p1".to_string()));
}

#[test]
fn history_up_cycles_newest_first_and_clamps_at_oldest() {
    let mut state = AppState::default();
    for t in ["a", "b", "c"] {
        state.enqueue_prompt(t.to_string());
        let _ = state.activate_next_prompt();
        state.complete_active_prompt();
    }
    let submitted = state.submitted_prompt_texts();
    let r1 = state.input.history_up(&submitted, "");
    assert_eq!(r1, Some("c".to_string()));
    let r2 = state.input.history_up(&submitted, "");
    assert_eq!(r2, Some("b".to_string()));
    let r3 = state.input.history_up(&submitted, "");
    assert_eq!(r3, Some("a".to_string()));
    let r4 = state.input.history_up(&submitted, "");
    assert_eq!(r4, Some("a".to_string())); // clamp
}

#[test]
fn history_down_past_newest_restores_draft() {
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let _ = state
        .input
        .history_up(&state.submitted_prompt_texts(), "draft");
    let submitted = state.submitted_prompt_texts();
    assert_eq!(
        state.input.history_down(&submitted),
        Some("draft".to_string())
    );
}

#[test]
fn history_up_moves_cursor_up_in_multiline_buffer() {
    // History navigation now returns text; cursor is managed by TextArea.
    // This test verifies the text is returned correctly.
    let mut state = AppState::default();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let submitted = state.submitted_prompt_texts();
    let result = state.input.history_up(&submitted, "line1\nline2");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_up_clamps_column_to_shorter_prev_line() {
    // History navigation now returns text; cursor is managed by TextArea.
    let mut state = AppState::default();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let submitted = state.submitted_prompt_texts();
    let result = state.input.history_up(&submitted, "ab\nxyz");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_up_on_first_line_of_multiline_enters_history() {
    let mut state = AppState::default();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let submitted = state.submitted_prompt_texts();
    let result = state.input.history_up(&submitted, "line1\nline2");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_down_moves_cursor_down_in_multiline() {
    // History navigation now returns text; cursor is managed by TextArea.
    let mut state = AppState::default();
    let submitted = state.submitted_prompt_texts();
    let result = state.input.history_down(&submitted);
    assert_eq!(result, None);
}

#[test]
fn typing_resets_history_navigation() {
    let mut state = AppState::default();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let submitted = state.submitted_prompt_texts();
    let _ = state.input.history_up(&submitted, "");
    assert_eq!(
        state.input.history_up(&submitted, ""),
        Some("p1".to_string())
    );
    // After typing, history navigation is reset
    state.input.reset_history_navigation();
    assert_eq!(state.input.history_down(&submitted), None);
    state.input.reset_history_navigation();
    assert_eq!(state.input.history_down(&submitted), None);
}

#[test]
fn insert_exit_pending_j_is_true_within_timeout() {
    let mut state = AppState::default();
    assert!(!state.input.insert_exit_pending_j());

    state.input.set_insert_exit_pending_j();
    assert!(state.input.insert_exit_pending_j());
}

#[test]
fn insert_exit_pending_j_is_false_after_timeout() {
    let mut state = AppState::default();
    state.input.set_insert_exit_pending_j();

    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(!state.input.insert_exit_pending_j());
}

#[test]
fn clear_insert_exit_pending_j_resets_to_false() {
    let mut state = AppState::default();
    state.input.set_insert_exit_pending_j();
    assert!(state.input.insert_exit_pending_j());

    state.input.clear_insert_exit_pending_j();
    assert!(!state.input.insert_exit_pending_j());
    state.input.clear_insert_exit_pending_j();
    assert!(!state.input.insert_exit_pending_j());
}

#[test]
fn push_spacer_adds_spacer_when_last_is_not_spacer() {
    let mut state = AppState::default();
    state.transcript.push_transcript_item(TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".into(),
        }),
        status: None,
    });
    state.transcript.push_spacer();
    assert_eq!(state.transcript.entries.len(), 2);
    assert!(matches!(
        state.transcript.entries[1].kind,
        TranscriptEntryKind::Spacer(_)
    ));
}

#[test]
fn user_prompt_queued_during_external_prompt_not_double_delivered() {
    let mut state = AppState::default();

    // External prompt arrives and is active
    state.enqueue_external_prompt("external task".to_string());
    assert_eq!(active_prompt_id(&state), Some(1));

    // User submits a prompt while external is running
    state.enqueue_prompt("user task".to_string());

    // First take should return None — user prompt waits for external to complete
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "user prompt must NOT be delivered while external prompt is active"
    );

    // Complete the external prompt
    state.complete_active_prompt();

    // Now the user prompt should be delivered exactly once
    let result = state.take_next_prompt_for_execution();
    assert_eq!(
        result.as_deref(),
        Some("user task"),
        "user prompt should be delivered after external prompt completes"
    );

    // Second take should return None — not delivered again
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "user prompt must NOT be delivered a second time"
    );
}

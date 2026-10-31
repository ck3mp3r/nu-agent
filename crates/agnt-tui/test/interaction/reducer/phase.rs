use super::*;

use agnt_core::protocol::events::{RequestId, SystemKind, ToolCall, ToolOutcome, Usage};
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::ir::ToolName;

#[test]
fn submit_and_lock_table() {
    // Submit transition is deterministic and keeps the input editable.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("status pods".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "status pods");

    // Locked input prevents typing and submission.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("first".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );

    state.input.pending_submit_text = Some("second".to_string());
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );

    // Activate both prompts coalesced
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, Some("first\nsecond".to_string()));
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "first\nsecond");
    // Complete first (active) prompt — other prompt is already Done
    state.complete_active_prompt();
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, None);

    // A whitespace-only prompt is a no-op.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("  \t\n ".to_string()),
        ..Default::default()
    };

    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(state.transcript.entries().is_empty());
}

#[test]
fn abort_flow_table() {
    // Esc then Esc confirm moves into abort requested without unlocking.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("do work".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();

    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);

    let before_markers = state.transcript.len();
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    assert!(state.status.message.status_line().is_empty());
    // cancel no longer pushes a spacer — the unified rule separates blocks
    assert_eq!(state.transcript.len(), before_markers);

    // A race: completion before the second escape prevents re-entry into abort pending.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("race".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();

    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert_eq!(state.phase, UiPhase::AbortPending);

    reduce_ui(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);

    let transcript_before = state.transcript.entries().to_vec();
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.transcript.entries(), transcript_before);
}

#[test]
fn test_interaction_reducer_turn_end_closes_permission_card() -> Result<()> {
    // -- Setup & Fixtures: a busy turn with a permission card up.
    let mut state = busy_state_with_clean_transcript();
    open_permission_card(&mut state);
    assert!(state.permission.has_prompt());

    // -- Exec: the turn completes.
    reduce_ui(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );

    // -- Check
    assert!(
        !state.permission.has_prompt(),
        "turn completion must close a surviving permission card"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "closing the card must not submit a decision"
    );

    // -- Setup & Fixtures: the same, ended by a turn error.
    let mut state = busy_state_with_clean_transcript();
    open_permission_card(&mut state);

    // -- Exec
    reduce_ui(
        &mut state,
        SystemEvent::SystemMessage {
            kind: SystemKind::TurnError,
            text: "boom".to_string(),
        },
    );

    // -- Check
    assert!(
        !state.permission.has_prompt(),
        "a turn error must close a surviving permission card"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "closing the card must not submit a decision"
    );
    Ok(())
}

#[test]
fn test_interaction_reducer_permission_timed_out_closes_matching_card() -> Result<()> {
    // -- Setup & Fixtures: a busy turn with a permission card up.
    let mut state = busy_state_with_clean_transcript();
    open_permission_card(&mut state);
    let open_request_id = RequestId::new("ask-0000000000000001");

    // -- Exec: a timeout for a different request.
    reduce_ui(
        &mut state,
        SystemEvent::PermissionDecisionTimedOut {
            request_id: RequestId::new("ask-0000000000000002"),
        },
    );

    // -- Check: the card stays up and no status message is set.
    assert!(
        state.permission.has_prompt(),
        "a timeout for another request must leave the card up"
    );
    assert!(
        state.status.message.status_line().is_empty(),
        "a timeout for another request must not set a status message"
    );

    // -- Exec: the timeout for the open card.
    reduce_ui(
        &mut state,
        SystemEvent::PermissionDecisionTimedOut {
            request_id: open_request_id,
        },
    );

    // -- Check
    assert!(
        !state.permission.has_prompt(),
        "the matching timeout must close the card"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "closing the card on timeout must not submit a decision"
    );
    assert!(
        state.status.message.status_line().contains("timed out"),
        "the timeout must be announced in the status line, got '{}'",
        state.status.message.status_line()
    );
    Ok(())
}

#[test]
fn completion_finalize_table() {
    // Intermediate events keep the cycle busy; Completed is the finalize boundary.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("prompt".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();

    let cases = vec![
        SystemEvent::InferenceStarted,
        SystemEvent::Tick,
        SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                "mcp",
                "{}",
                CallLine::from_json_summary("{}"),
            ),
        },
        SystemEvent::ToolCompleted {
            call: ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                "mcp",
                "{}",
                CallLine::from_json_summary("{}"),
            ),
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "[]".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        },
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 12,
                tool_calls: 1,
                input_tokens: 4,
                output_tokens: 8,
                total_tokens: Some(12),
                cost: None,
                cost_complete: false,
            },
        },
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            text: "warned".to_string(),
        },
    ];

    for event in cases {
        reduce_ui(&mut state, event);
        assert_eq!(state.phase, UiPhase::Busy);
        assert!(!state.input_locked);
    }

    reduce_ui(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);

    // Completed event clears pending and unlocks input.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("do work".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);

    reduce_ui(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!state.input_locked);
    assert!(state.status.message.status_line().is_empty());

    // Completed event unlocks and clears abort pending.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("finalize".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);

    reduce_ui(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());

    // A turn error leaves the UI in a recoverable state.
    let mut state = busy_state_with_clean_transcript();

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(active_prompt_id(&state).is_some());
    assert!(state.is_active_cycle());

    reduce_ui(
        &mut state,
        SystemEvent::SystemMessage {
            kind: SystemKind::TurnError,
            text: "test error".to_string(),
        },
    );

    assert_eq!(active_prompt_id(&state), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert!(!state.abort.pending);

    let has_error_in_transcript = state
        .transcript
        .entries()
        .iter()
        .any(|entry| entry_text(entry).contains("test error"));
    assert!(
        has_error_in_transcript,
        "Expected error message in transcript"
    );

    state.input.pending_submit_text = Some("retry".to_string());
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    assert_eq!(state.phase, UiPhase::Busy);
    state.input_locked = true;
    let _ = state.take_next_prompt_for_execution();
    assert!(
        state
            .transcript
            .entries()
            .iter()
            .any(|entry| entry_text(entry) == "retry"),
        "Second prompt should appear in transcript"
    );

    assert_reducer_invariants(&state);
}

//! Golden event-log scenarios for `reduce` — the event → state seam.
//!
//! Each test is a named session scenario: the event sequence is the input, the
//! asserted state (and the drained effect queue) is the expectation. No test
//! here pokes reducer internals; every assertion reads a public state accessor
//! or the effect queue, exactly as production does.

use crate::interaction::{cancel::CancelController, reducer::reduce};
use crate::state::{AppState, InputMode, UiPhase};
use agnt_core::orchestrator::OrchestratorEvent;
use agnt_core::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplay, ToolDisplaySection,
};
use agnt_core::protocol::events::{Key, RequestId, SystemEvent, ToolCall, ToolOutcome, Usage};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{ContentKind, MessageRole, ToolName};
use agnt_core::transcript::renderer::ItemStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

fn tool_display(title: &str, content: &str) -> ToolDisplay {
    ToolDisplay {
        title: Some(title.to_string()),
        sections: vec![ToolDisplaySection {
            label: Some(title.to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: content.to_string(),
            stats: None,
        }],
    }
}

fn edit_call(arguments: &str) -> ToolCall {
    ToolCall::new(
        ToolName("edit".to_string()),
        "closure",
        arguments,
        CallLine::from_json_summary(arguments),
    )
}

fn usage(input_tokens: u64, output_tokens: u64) -> Usage {
    Usage {
        response_chars: 0,
        tool_calls: 0,
        input_tokens,
        output_tokens,
        total_tokens: Some(input_tokens + output_tokens),
        cost: None,
        cost_complete: true,
    }
}

fn inference_chunk(delta: &str, aggregated: &str) -> SystemEvent {
    SystemEvent::InferenceChunk {
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
}

/// The raw markdown of every assistant message entry, in transcript order.
fn assistant_markdown(state: &AppState) -> Vec<String> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::Assistant => {
                Some(message.markdown.clone())
            }
            _ => None,
        })
        .collect()
}

/// Fold one event log into a fresh state, draining the prompt queue between
/// events the way the coordinator does. Replay and live reduction must agree.
fn replay(log: &[SystemEvent]) -> AppState {
    let mut state = AppState::default();
    for event in log {
        reduce(&mut state, event.clone(), None);
        let _ = state.take_next_prompt_for_execution();
    }
    state
}

// endregion: --- Fixtures

// region:    --- Scenarios

/// Prompt submit → inference start → streaming chunks → turn completion.
///
/// The log carries realistic payloads: each chunk's `delta` is the new suffix,
/// its `aggregated` the accumulated text. The scenario snapshots the streamed
/// block AFTER EACH CHUNK, so a reducer that keeps only the latest chunk fails
/// here — the block must only ever grow.
#[test]
fn golden_log_prompt_submit_streaming_and_turn_completion() -> Result<()> {
    // -- Setup & Fixtures
    let cancel_controller = CancelController::default();
    let log = vec![
        SystemEvent::PromptSubmitted {
            text: "say hi".to_string(),
        },
        SystemEvent::InferenceStarted,
        inference_chunk("hi", "hi"),
        inference_chunk(" there", "hi there"),
        SystemEvent::InferenceCompleted { usage: usage(3, 2) },
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: Some(5),
            cost: None,
            cost_complete: true,
            duration_ms: 12,
        },
    ];

    // -- Exec: the submit folds, the coordinator drains the effect and hands
    // the prompt to the turn executor, then the inference events arrive.
    let mut state = AppState::default();
    reduce(&mut state, log[0].clone(), Some(&cancel_controller));
    let effects = state.take_pending_events(&cancel_controller);
    let _ = state.take_next_prompt_for_execution();
    reduce(&mut state, log[1].clone(), Some(&cancel_controller));

    // -- Check: the submit is an outbound effect, not a state write.
    assert_eq!(effects.len(), 1, "exactly one outbound prompt");
    assert!(matches!(
        effects.first(),
        Some(OrchestratorEvent::PromptSubmitted { text }) if text == "say hi"
    ));

    // -- Exec & Check: the first chunk renders its own text — the block is
    // visible before the stream completes.
    reduce(&mut state, log[2].clone(), Some(&cancel_controller));
    assert_eq!(
        assistant_markdown(&state),
        vec!["hi".to_string()],
        "after chunk 1"
    );

    // -- Exec & Check: the second chunk grows the same block to the accumulated
    // text; chunk 1's text is still there.
    reduce(&mut state, log[3].clone(), Some(&cancel_controller));
    assert_eq!(
        assistant_markdown(&state),
        vec!["hi there".to_string()],
        "after chunk 2"
    );

    // -- Exec: the turn closes.
    for event in &log[4..] {
        reduce(&mut state, event.clone(), Some(&cancel_controller));
    }

    // -- Check: the turn closed cleanly and holds the final streamed text.
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert_eq!(
        assistant_markdown(&state),
        vec!["hi there".to_string()],
        "one block, the accumulated text"
    );

    // -- Check: replay determinism — the same log yields the same state.
    let replayed = replay(&log);
    assert_eq!(assistant_markdown(&replayed), assistant_markdown(&state));
    assert_eq!(replayed.phase, state.phase);
    Ok(())
}

/// Tool start → permission-gate preview → completion: the call keeps the
/// ONE-ENTRY shape (the preview and the terminal status live on the entry)
/// for its whole life.
#[test]
fn golden_log_tool_with_preview_keeps_one_entry_shape() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = r#"{"path":"README.md"}"#;
    let call = edit_call(arguments);
    let log = vec![
        SystemEvent::ToolStarted { call: call.clone() },
        SystemEvent::PreviewRequested {
            call: call.clone(),
            display: tool_display("edit README.md", "--- a\n+++ b\n"),
        },
        SystemEvent::ToolCompleted {
            call,
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        },
    ];

    // -- Exec
    let state = replay(&log);

    // -- Check: ONE tool entry carrying the preview and the terminal status.
    assert_eq!(
        state.transcript.len(),
        1,
        "the preview lives on the tool entry"
    );
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have the tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(tool.preview.is_some(), "the preview must be set in place");
    assert_eq!(tool.status, ItemStatus::Done);
    Ok(())
}

/// Tool start → permission-gate preview → completion WITH a display: the store
/// holds exactly ONE tool entry carrying both the preview and the completion
/// result, with the terminal status.
#[test]
fn golden_log_tool_lifecycle_holds_one_entry_with_preview_and_result() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = r#"{"path":"README.md"}"#;
    let call = edit_call(arguments);
    let log = vec![
        SystemEvent::ToolStarted { call: call.clone() },
        SystemEvent::PreviewRequested {
            call: call.clone(),
            display: tool_display("edit README.md", "--- a\n+++ b\n"),
        },
        SystemEvent::ToolCompleted {
            call,
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display: Some(tool_display("edit README.md", "--- a\n+++ b\n")),
                error_kind: None,
                message: None,
            },
        },
    ];

    // -- Exec
    let state = replay(&log);

    // -- Check: exactly one ToolEntry, preview and result both set.
    let tool_entries: Vec<_> = state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool),
            _ => None,
        })
        .collect();
    assert_eq!(tool_entries.len(), 1, "one tool call, one entry");
    let tool = tool_entries.first().ok_or("should have the tool entry")?;
    assert!(tool.preview.is_some(), "the gate preview stays on entry");
    assert!(tool.result.is_some(), "the completion result is recorded");
    assert_eq!(tool.status, ItemStatus::Done);
    Ok(())
}

/// A permission decision writes exactly one outbound decision for the open
/// request, and closes the card.
#[test]
fn golden_log_permission_decision_writes_one_effect() -> Result<()> {
    // -- Setup & Fixtures
    let cancel_controller = CancelController::default();
    let arguments = r#"{"path":"a.rs"}"#;
    let call = edit_call(arguments);
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::ToolStarted { call: call.clone() },
        Some(&cancel_controller),
    );
    reduce(
        &mut state,
        SystemEvent::PermissionRequested {
            request_id: RequestId::new("ask-1"),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                tool_key: call.id.as_str().to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "→ {...}".to_string(),
                pre_authorize_display: Some(tool_display("edit a.rs", "--- a\n+++ b\n")),
            },
        },
        Some(&cancel_controller),
    );

    // -- Check: the gate opened and queued nothing yet.
    assert!(state.permission.has_prompt(), "the card is up");
    assert!(state.take_pending_events(&cancel_controller).is_empty());

    // -- Exec: the user allows the call once.
    let changed = reduce(
        &mut state,
        SystemEvent::PermissionDecided {
            decision: PermissionDecision::AllowOnce,
        },
        Some(&cancel_controller),
    );
    let effects = state.take_pending_events(&cancel_controller);

    // -- Check
    assert!(changed, "a decision is a visible change");
    assert!(
        !state.permission.has_prompt(),
        "the card closes on decision"
    );
    assert_eq!(effects.len(), 1, "exactly one permission decision");
    assert!(matches!(
        effects.first(),
        Some(OrchestratorEvent::PermissionDecision { decision })
            if decision.decision == PermissionDecision::AllowOnce
                && decision.request_id == "ask-1"
    ));
    Ok(())
}

// endregion: --- Scenarios

// region:    --- Key-path scenarios

/// The insert-exit chord: one `j` arms the chord, the second one leaves
/// insert mode. The same fold must hold when the keymap lives in the reducer
/// instead of the input lens.
#[test]
fn golden_key_insert_exit_chord_reaches_normal_mode() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert, "starts in insert mode");

    // -- Exec: the first `j` only arms the chord.
    let first = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );

    // -- Check: arming the chord edits nothing.
    assert!(!first, "the armed chord reports no change");
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec: the second `j` is the chord.
    let second = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );

    // -- Check
    assert!(second, "the chord transition is a visible change");
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}

/// Normal-mode navigation: `g` arms the pending key and the second `g` jumps
/// to the top of the transcript.
#[test]
fn golden_key_normal_pending_g_scrolls_to_top() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: crate::state::InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    state.scroll.viewport_height = 10;
    state.scroll.cursor_visual_row = 5;
    state.scroll.total_visual_rows = 30;

    // -- Exec: the first `g` only arms the pending key.
    let first = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('g'),
        },
        None,
    );

    // -- Check: arming the pending key reports no change.
    assert!(!first, "the armed pending key reports no change");
    assert_eq!(state.scroll.cursor_visual_row, 5);

    // -- Exec: the second `g` scrolls to the top.
    let second = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('g'),
        },
        None,
    );

    // -- Check
    assert!(second);
    assert_eq!(state.scroll.cursor_visual_row, 0);
    Ok(())
}

/// A busy-mode Esc leaves insert mode and arms the turn-cancel confirmation.
#[test]
fn golden_key_busy_escape_arms_abort_confirmation() -> Result<()> {
    // -- Setup & Fixtures: a running turn, input locked.
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "say hi".to_string(),
        },
        None,
    );
    let _ = state.take_next_prompt_for_execution();
    reduce(&mut state, SystemEvent::InferenceStarted, None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);

    // -- Check: mode left insert and the abort confirmation is armed.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert_eq!(
        state.status.message.status_line(),
        crate::interaction::reducer::ESC_ABORT_CONFIRM_STATUS
    );
    Ok(())
}

/// The permission card owns its decision keys: `a` writes exactly one
/// AllowOnce decision through the key path.
#[test]
fn golden_key_permission_card_decision() -> Result<()> {
    // -- Setup & Fixtures
    let cancel_controller = CancelController::default();
    let arguments = r#"{"path":"a.rs"}"#;
    let call = edit_call(arguments);
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::ToolStarted { call: call.clone() },
        Some(&cancel_controller),
    );
    reduce(
        &mut state,
        SystemEvent::PermissionRequested {
            request_id: RequestId::new("ask-1"),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                tool_key: call.id.as_str().to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "→ {...}".to_string(),
                pre_authorize_display: None,
            },
        },
        Some(&cancel_controller),
    );
    assert!(state.permission.has_prompt(), "the card is up");

    // -- Exec
    let changed = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('a'),
        },
        Some(&cancel_controller),
    );
    let effects = state.take_pending_events(&cancel_controller);

    // -- Check
    assert!(changed);
    assert!(
        !state.permission.has_prompt(),
        "the card closes on decision"
    );
    assert_eq!(effects.len(), 1, "exactly one permission decision");
    assert!(matches!(
        effects.first(),
        Some(OrchestratorEvent::PermissionDecision { decision })
            if decision.decision == PermissionDecision::AllowOnce
                && decision.request_id == "ask-1"
    ));
    Ok(())
}

// endregion: --- Key-path scenarios

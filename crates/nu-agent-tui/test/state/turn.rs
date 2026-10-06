//! Turn-domain reducer tests: turn completion finalizes the cycle and closes
//! the transcript block. Assertions moved 1:1 from the former
//! `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests, driven
//! through `dispatch_turn_event` (the single turn dispatch seam).

use crate::interaction::reducer::{ReducerInput, UserAction, reduce_with_cancel_controller};
use crate::state::{AppState, InputState, StatusMessageKind, UiPhase};
use nu_agent_core::bus::TurnEvent;
use nu_agent_core::transcript::ir::{Block, BlockSource, MessageRole};
use nu_agent_core::transcript::items::Message;
use nu_agent_core::transcript::renderer::Renderable;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn finalize_turn(state: &mut AppState) {
    crate::state::dispatch_turn_event(state, TurnEvent::Completed { tool_calls: 0 });
}

#[test]
fn finalize_keeps_last_block_content_for_unified_rule() -> Result<()> {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("prompt".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "response".to_string(),
    };
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });

    // Dispatch the turn Completed event which calls finalize
    finalize_turn(&mut state);

    // No explicit spacer is pushed at finalize; the last block stays content.
    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    assert!(matches!(last.source, BlockSource::Markdown { .. }));
    Ok(())
}

#[test]
fn completed_event_finalizes_cycle_and_unlocks_input() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("finalize".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    finalize_turn(&mut state);

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn turn_started_and_task_outcomes_are_render_noops() {
    let mut state = AppState::default();

    assert!(!crate::state::dispatch_turn_event(
        &mut state,
        TurnEvent::Started {
            prompt: "p".to_string(),
            task_id: None,
        },
    ));
    assert!(!crate::state::dispatch_turn_event(
        &mut state,
        TurnEvent::TaskCompleted {
            output: "out".to_string(),
            task_id: "t".to_string(),
        },
    ));
    assert!(!crate::state::dispatch_turn_event(
        &mut state,
        TurnEvent::TaskFailed {
            task_id: "t".to_string(),
            error: "boom".to_string(),
        },
    ));
}

#[test]
fn finalize_resets_assistant_stream_start() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(3);

    finalize_turn(&mut state);

    assert!(state.transcript.assistant_stream_start.is_none());
}

/// Fix 1 (deterministic): `finalize` clears only Neutral-kind status messages.
/// A warning-kind message survives turn completion and expires via its own
/// TTL — the repetition-stop warning arrives just before `TurnEvent::Completed`
/// and must not be wiped by it.
#[test]
fn finalize_preserves_warning_and_clears_neutral_status() -> Result<()> {
    // -- Setup & Fixtures: a warning-kind status message.
    let mut state = AppState::default();
    state
        .status
        .message
        .set_warning("Output repetition stopped: …");

    // -- Exec
    finalize_turn(&mut state);

    // -- Check: the warning survives.
    assert_eq!(
        state.status.message.status_line(),
        "Output repetition stopped: …",
        "finalize must preserve a warning-kind status message"
    );
    assert_eq!(
        state.status.message.kind(),
        StatusMessageKind::Warning,
        "the surviving message must still be warning-kind"
    );

    // -- Setup & Fixtures: a Neutral-kind status message.
    let mut state = AppState::default();
    state.status.message.set_message("Tool: prior");

    // -- Exec
    finalize_turn(&mut state);

    // -- Check: the neutral message is cleared.
    assert!(
        state.status.message.status_line().is_empty(),
        "finalize must clear a Neutral-kind status message"
    );

    Ok(())
}

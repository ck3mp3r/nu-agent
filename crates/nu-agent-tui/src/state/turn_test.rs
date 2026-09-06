//! Turn-domain reducer tests: turn completion finalizes the cycle and closes
//! the transcript block. Assertions moved 1:1 from the former
//! `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests, driven
//! through `dispatch_turn_event` (the single turn dispatch seam).

use crate::interaction::reducer::{ReducerInput, UserAction, reduce_with_cancel_controller};
use crate::state::{AppState, InputState, UiPhase};
use nu_agent_core::bus::TurnEvent;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn finalize_turn(state: &mut AppState) {
    crate::state::dispatch_turn_event(state, TurnEvent::Completed { tool_calls: 0 });
}

#[test]
fn finalize_pushes_closing_spacer() -> Result<()> {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("prompt".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.push_transcript_item(TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "response".to_string(),
        }),
        status: None,
    });

    // Dispatch the turn Completed event which calls finalize
    finalize_turn(&mut state);

    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    assert!(matches!(last.kind, TranscriptEntryKind::Spacer(_)));
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
fn turn_started_and_task_completed_are_render_noops() {
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
}

#[test]
fn finalize_resets_assistant_stream_start() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(3);

    finalize_turn(&mut state);

    assert!(state.transcript.assistant_stream_start.is_none());
}

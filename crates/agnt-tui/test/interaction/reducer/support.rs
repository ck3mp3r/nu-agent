use super::*;

use ratatui::layout::Rect;

/// Push one user Message block via the store's sole push API.
pub(super) fn push_user_line(state: &mut AppState, text: impl Into<String>) {
    let msg = Message {
        role: MessageRole::User,
        markdown: text.into(),
    };
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });
}

/// Convenience: wraps a UiEvent into a boxed ReducerInput::Event.
pub(super) fn event_input(e: UiEvent) -> ReducerInput {
    ReducerInput::Event(Box::new(e))
}

pub(super) fn assert_reducer_invariants(state: &AppState) {
    match state.phase {
        UiPhase::Idle => assert!(!state.input_locked),
        UiPhase::Busy | UiPhase::AbortPending => assert!(state.input_locked),
    }
    assert_eq!(state.abort.pending, state.phase == UiPhase::AbortPending);
    if state.phase == UiPhase::Idle {
        assert!(!state.is_active_cycle());
    }
}

/// The active prompt is the one currently in `InProgress` status.
pub(super) fn active_prompt_id(state: &AppState) -> Option<u64> {
    state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .map(|p| p.id)
}

/// Pending prompts are those still in `Queued` status.
pub(super) fn pending_prompt_ids(state: &AppState) -> Vec<u64> {
    state
        .prompt_items()
        .iter()
        .filter(|p| p.status == PromptStatus::Queued)
        .map(|p| p.id)
        .collect()
}

pub(super) fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("run".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

/// App state for wheel-reducer tests: a rendered transcript area, the cursor at
/// the top visual row, and nothing to scroll up from. The pane focus follows the
/// input-mode invariant.
pub(super) fn wheel_noop_state(mode: InputMode) -> AppState {
    let mut state = AppState {
        input: InputState::default().with_mode(mode),
        scroll: ScrollState {
            pane_focus: if mode == InputMode::Insert {
                PaneFocus::Input
            } else {
                PaneFocus::Transcript
            },
            viewport_height: 10,
            cursor_visual_row: 0,
            total_visual_rows: 30,
            scroll_offset: 0,
            max_scroll: 0,
            following_tail: false,
            entry_indices: (0..30).collect(),
            transcript_area: Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    state
}

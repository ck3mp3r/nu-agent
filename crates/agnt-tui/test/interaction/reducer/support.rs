use super::*;

use ratatui::layout::Rect;

/// Push one user message entry via the store's sole push API.
pub(super) fn push_user_line(state: &mut AppState, text: impl Into<String>) {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: text.into(),
    });
    state.transcript.push_entry(entry);
}

/// Fold one `SystemEvent` through the boundary shim and the reducer.
pub(super) fn reduce_ui(state: &mut AppState, event: SystemEvent) -> bool {
    reduce(state, event, None)
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

/// Open a permission card on the state, as a `PermissionRequested` event would.
pub(super) fn open_permission_card(state: &mut AppState) {
    state
        .permission
        .open_prompt(crate::state::PermissionPrompt {
            request_id: "ask-0000000000000001".to_string(),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            scope: "nested".to_string(),
            pattern: "*".to_string(),
            target_field: Some("command".to_string()),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
        });
}

pub(super) fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "run".to_string(),
        },
        None,
    );
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

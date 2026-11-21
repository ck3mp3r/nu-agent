use super::*;
use crate::interaction::reducer::{RenderInput, reduce};
use crate::test_support::entry_text;
use agnt_runtime::bus::ControlCommand;
use agnt_runtime::protocol::contracts::SharedUiAction;

#[test]
fn submit_path_appends_prompt_and_keeps_input_editable() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("s".to_string()),
        ..Default::default()
    };

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "s");
}

#[test]
fn dispatch_edit_actions_are_noops() {
    // Backspace and cursor movement are handled by TextArea, not the dispatch path.
    let mut state = AppState::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert!(!changed);

    // Alt+Enter and Shift+Enter are handled by the coordinator (TextArea),
    // not the dispatch path. The dispatch path's InsertNewline is a no-op.
    let mut state = AppState::default();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::AltEnter), None);
    assert!(!changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(state.transcript.entries().is_empty());

    // Enter still submits via pending_submit_text
    state.input.pending_submit_text = Some("h".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]).trim(), "h");
}

/// WHEN `/stats` is submitted from the input, THE handle_submit arm SHALL flip
/// `render_stats_visible` through the local launch queue and SHALL enqueue
/// nothing for the orchestrator: no prompt, no immediate submission, and no
/// echo of the command text.
#[test]
fn test_interaction_submit_stats_toggles_without_enqueueing() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    assert!(!state.render_stats_visible, "the popup starts hidden");

    // -- Exec: one typed submit.
    state.input = InputState::default().with_pending_submit_text("/stats".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the toggle applied, and the command went nowhere else.
    assert!(changed);
    assert!(
        state.render_stats_visible,
        "the first submit shows the popup"
    );
    assert_eq!(state.phase, UiPhase::Idle, "no turn starts");
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "nothing is enqueued for the orchestrator"
    );
    assert_eq!(state.pending_prompt_count(), 0, "no prompt is queued");
    assert!(state.prompt_items().is_empty());
    assert!(
        state.transcript.entries().is_empty(),
        "the text is not echoed"
    );
    Ok(())
}

/// WHEN `/stats` runs while `render_stats_visible` is true, THE dispatch SHALL
/// make it false — the same command toggles both ways, with no argument form.
#[test]
fn test_interaction_submit_stats_toggles_back_off() -> Result<()> {
    // -- Setup & Fixtures: the popup showing.
    let mut state = AppState {
        render_stats_visible: true,
        ..Default::default()
    };

    // -- Exec: the same submit again.
    state.input = InputState::default().with_pending_submit_text("/stats".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check
    assert!(changed);
    assert!(!state.render_stats_visible, "the second submit hides it");
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(
        state.transcript.entries().is_empty(),
        "the text is not echoed"
    );
    Ok(())
}

/// WHEN the inline-suggestion accept path submits the selected `/stats` row,
/// THE same handler SHALL toggle the popup: the accept lane submits the label,
/// never typed argument text.
#[test]
fn test_interaction_submit_stats_accepts_from_inline_suggestions() -> Result<()> {
    // -- Setup & Fixtures: `/st` narrows the list onto the two `stat` rows.
    let mut state = AppState::default();
    state.check_inline_slash("/st");
    assert_eq!(
        state.picker.selected_slash_label(),
        Some("/stats".to_string())
    );

    // -- Exec: accept the highlighted suggestion.
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // -- Check
    assert!(changed);
    assert!(state.render_stats_visible);
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert!(
        state.transcript.entries().is_empty(),
        "the text is not echoed"
    );
    Ok(())
}

/// WHEN the surface folds ControlCommand::SharedUiAction { RenderStats }, THE
/// state SHALL flip the popup exactly once per action.
#[test]
fn test_interaction_submit_stats_control_action_flips_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let command = ControlCommand::SharedUiAction {
        action: SharedUiAction::RenderStats,
    };

    // -- Exec & Check: the first fold queues the action and the drain applies it.
    assert!(reduce(&mut state, RenderInput::Control(&command), None));
    assert!(state.drain_launch_requests());
    assert!(state.render_stats_visible, "one action shows the popup");

    // -- Exec & Check: the second fold flips it back, once.
    assert!(reduce(&mut state, RenderInput::Control(&command), None));
    assert!(state.drain_launch_requests());
    assert!(!state.render_stats_visible, "one action hides the popup");
    Ok(())
}

/// THE HUD SHALL NOT bind, consume, or alter Esc: with the popup showing, one
/// Esc leaves the surface in exactly the state it leaves it in hidden.
#[test]
fn test_interaction_submit_stats_esc_keeps_its_contract() -> Result<()> {
    // -- Setup & Fixtures: two idle surfaces, one showing the popup.
    let mut hidden = AppState::default();
    let mut shown = AppState::default();
    assert!(shown.execute_shared_ui_action(SharedUiAction::RenderStats));
    assert!(shown.render_stats_visible);

    // -- Exec: one bare Esc over each surface.
    let hidden_changed =
        dispatch_terminal_event(&mut hidden, &TerminalEvent::Key(TerminalKey::Esc), None);
    let shown_changed =
        dispatch_terminal_event(&mut shown, &TerminalEvent::Key(TerminalKey::Esc), None);

    // -- Check: one outcome, and the press never touches the popup.
    assert_eq!(hidden_changed, shown_changed);
    assert_eq!(hidden.input.mode, shown.input.mode);
    assert_eq!(hidden.phase, shown.phase);
    assert_eq!(hidden.abort.pending, shown.abort.pending);
    assert_eq!(
        hidden.status.message.status_line(),
        shown.status.message.status_line()
    );
    assert!(shown.render_stats_visible, "Esc does not hide the popup");
    Ok(())
}

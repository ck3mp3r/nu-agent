use super::*;
use crate::interaction::reducer::{RenderInput, reduce};
use crate::state::StatusMessageKind;
use crate::test_support::entry_text;
use agnt_runtime::bus::{CommandFacts, CompactionAvailability, ControlCommand, FactRecord};
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

/// WHEN `/compact` is submitted while the runtime answers that nothing is left to
/// cut, THEN THE submit lane SHALL refuse it with that reason and SHALL enqueue
/// nothing: no turn starts, nothing reaches the orchestrator, and the command
/// text is not echoed.
#[test]
fn test_interaction_submit_refuses_an_unavailable_compact_with_the_reason() -> Result<()> {
    // -- Setup & Fixtures: the runtime's answer, folded as the surface receives it.
    let reason = "the conversation ends with its most recent compaction.";
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(CommandFacts {
                compact: CompactionAvailability::Unavailable(reason.to_string()),
                ..CommandFacts::default()
            }))
    );
    state.input = InputState::default().with_pending_submit_text("/compact".to_string());

    // -- Exec: one typed submit.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the refusal is the reason the surface carried, in a warning line.
    assert!(changed);
    assert_eq!(state.status.message.status_line(), reason);
    assert_eq!(state.status.message.kind(), StatusMessageKind::Warning);

    // -- Check: nothing was submitted, and nothing is echoed.
    assert_eq!(state.phase, UiPhase::Idle, "no turn starts");
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "a refused command SHALL NOT reach the orchestrator"
    );
    assert_eq!(state.pending_prompt_count(), 0, "no prompt is queued");
    assert!(
        state.transcript.entries().is_empty(),
        "the command text is not echoed"
    );
    Ok(())
}

/// WHEN `/uncompact` is submitted while the runtime answers that no compaction
/// waits to be undone, THEN THE submit lane SHALL refuse it with that reason and
/// SHALL enqueue nothing: the undo's gate is the same seam as the sweep's.
#[test]
fn test_interaction_submit_refuses_an_unavailable_uncompact_with_the_reason() -> Result<()> {
    // -- Setup & Fixtures: the runtime's answer, folded as the surface receives it.
    let reason = "the conversation holds no compaction.";
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(CommandFacts {
                uncompact: CompactionAvailability::Unavailable(reason.to_string()),
                ..CommandFacts::default()
            }))
    );
    state.input = InputState::default().with_pending_submit_text("/uncompact".to_string());

    // -- Exec: one typed submit.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the refusal is the reason the surface carried, in a warning line.
    assert!(changed);
    assert_eq!(state.status.message.status_line(), reason);
    assert_eq!(state.status.message.kind(), StatusMessageKind::Warning);

    // -- Check: nothing was submitted, and nothing is echoed.
    assert_eq!(state.phase, UiPhase::Idle, "no undo starts");
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "a refused command SHALL NOT reach the orchestrator"
    );
    assert_eq!(state.pending_prompt_count(), 0, "no prompt is queued");
    assert!(
        state.transcript.entries().is_empty(),
        "the command text is not echoed"
    );
    Ok(())
}

/// WHEN the runtime answers that a sweep has work to do, THE same submit SHALL
/// proceed: `/compact` reaches the orchestrator on the immediate lane.
#[test]
fn test_interaction_submit_dispatches_an_available_compact() -> Result<()> {
    // -- Setup & Fixtures: the runtime answers that both commands have work.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("/compact".to_string()),
        ..Default::default()
    };
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(
                crate::test_support::both_commands_available()
            ))
    );

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the command went out, and no block or prompt row was invented.
    assert!(changed);
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("/compact".to_string()),
        "an available command reaches the orchestrator"
    );
    assert!(
        state.transcript.entries().is_empty(),
        "the command text is not echoed"
    );
    Ok(())
}

/// WHEN the availability changes after the suggestion rows were built, THEN THE
/// accept lane SHALL still refuse the command with the reason: the reducer's
/// refusal is the race backstop, and the rows were drawn from the older answer.
#[test]
fn test_interaction_submit_refuses_an_unavailable_compact_from_inline_suggestions() -> Result<()> {
    // -- Setup & Fixtures: `/c` narrows the list onto `/compact` while the
    // runtime still answers that the command can run, so the row is built as
    // available.
    let reason = "the conversation ends with its most recent compaction.";
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(
                crate::test_support::both_commands_available()
            ))
    );
    state.check_inline_slash("/c");
    assert_eq!(
        state.picker.selected_slash_label(),
        Some("/compact".to_string())
    );

    // -- Exec: the runtime's answer arrives after the rows were built, then the
    // user accepts the stale row.
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(CommandFacts {
                compact: CompactionAvailability::Unavailable(reason.to_string()),
                ..CommandFacts::default()
            }))
    );
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // -- Check: the submit lane refuses with the reason the surface carries, and
    // nothing reaches the orchestrator.
    assert!(changed);
    assert_eq!(state.status.message.status_line(), reason);
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "a refused suggestion SHALL NOT reach the orchestrator"
    );
    Ok(())
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

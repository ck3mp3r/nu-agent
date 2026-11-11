//! The queue-less Esc route: a peer turn the surface never submitted.
//!
//! The defect the active-cycle widening repairs (state/lifecycle.rs:209-215):
//! while a cycle was keyed on the prompt queue alone, a peer turn left the
//! surface busy through an `InferenceStarted` fold the queue never saw. The
//! first Esc found no cycle behind the arm guard, dropped through, and showed
//! no status line — the press died silently, and only Ctrl-C quit ended the
//! turn, and it ended the session with it. The widened cycle counts the input
//! lock a live turn holds; these tests pin the arm and the confirm on a
//! queue-less busy surface.

use std::time::Duration;

use super::*;
use crate::test_support::{fold_record, inference_started, peer_prompt_record, turn_completed};
use agnt_runtime::orchestrator::events::OrchestratorEvent;

/// WHEN a peer turn streams — the peer's row and an `InferenceStarted` folded
/// on an empty queue — THE `handle_escape` reducer SHALL arm on the first Esc
/// press: the phase is AbortPending, the arm is pending, the status line is
/// the confirm hint, and the CancelController stays unflagged.
#[test]
fn test_interaction_abort_cancel_queue_less_arms_on_a_peer_turn() -> Result<()> {
    // -- Setup & Fixtures: a peer turn the surface never submitted.
    let (mut state, cancel_controller) = queue_less_peer_turn();
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.input_locked, "a live turn holds the input lock");
    assert!(
        state.prompt_items().is_empty(),
        "the peer's row renders without entering the queue"
    );

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the arm, not a silent press.
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(
        !cancel_controller.is_cancel_requested(),
        "the arm presses nothing: the confirm press owns the cancel"
    );
    Ok(())
}

/// WHEN the arm is live on a queue-less busy turn, THE `handle_escape_confirm`
/// path SHALL flag the CancelController on the second Esc press and settle the
/// surface idle with an empty queue, and THE `AppState::take_pending_events`
/// function SHALL emit `OrchestratorEvent::CancelRequested`.
#[test]
fn test_interaction_abort_cancel_queue_less_confirms_on_the_second_press() -> Result<()> {
    // -- Setup & Fixtures: a peer turn armed by one Esc.
    let (mut state, cancel_controller) = queue_less_peer_turn();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the confirm ran, the surface settles idle, and the cancel
    // reaches the loop.
    assert!(changed);
    assert!(
        cancel_controller.is_cancel_requested(),
        "the confirm press flags the cancel"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
    assert_eq!(state.input.mode, InputMode::Insert);
    assert!(
        state.prompt_items().is_empty(),
        "the queue holds nothing the peer turn created"
    );
    let events = state.take_pending_events(&cancel_controller);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::CancelRequested)),
        "the confirmed cancel must reach the loop as a CancelRequested event"
    );
    Ok(())
}

/// WHEN the arm goes stale on a queue-less busy turn, THE next Esc press SHALL
/// restore `phase == Busy` first and SHALL re-arm: the phase is AbortPending
/// again with a fresh status line, not an insert-mode exit.
#[test]
fn test_interaction_abort_cancel_queue_less_stale_arm_rearms_while_busy() -> Result<()> {
    // -- Setup & Fixtures: a peer turn armed by one Esc, its arm window
    // already over.
    let (mut state, cancel_controller) = queue_less_peer_turn();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);
    let stale_at = state.abort.armed_at.ok_or("should have an arm stamp")?;
    state.abort.arm_window = Duration::ZERO;

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the sweep restored the streaming turn to its busy state, and
    // the press re-armed instead of leaving insert mode.
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(
        state.abort.pending,
        "the stale arm is replaced by a fresh one"
    );
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert_eq!(state.input.mode, InputMode::Normal);
    let rearmed_at = state
        .abort
        .armed_at
        .ok_or("should have armed the abort again")?;
    assert!(
        rearmed_at > stale_at,
        "the re-arm stamps a fresh instant, not the aged one"
    );
    assert!(
        !cancel_controller.is_cancel_requested(),
        "re-arming still owns the arm: the cancel waits for the next press"
    );
    Ok(())
}

/// WHEN a queue-less busy turn settles while an abort is armed, THE
/// `complete_active_prompt` settle SHALL owe the notice: the next idle Esc
/// press shows `ESC_SETTLED_STATUS` and requests no cancel.
#[test]
fn test_interaction_abort_cancel_queue_less_settle_owes_the_notice() -> Result<()> {
    // -- Setup & Fixtures: a peer turn armed by one Esc, then the turn settles
    // through the completed fold.
    let (mut state, cancel_controller) = queue_less_peer_turn();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);
    let settled = fold_record(&mut state, turn_completed(15));
    assert!(settled);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the settled notice, and no cancel anywhere in the effects.
    assert!(changed, "the notice must repaint");
    assert_eq!(state.status.message.status_line(), ESC_SETTLED_STATUS);
    assert!(
        !cancel_controller.is_cancel_requested(),
        "an arm lost to a settle must not cancel retroactively"
    );
    let events = state.take_pending_events(&cancel_controller);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::CancelRequested)),
        "the settled turn leaves no cancel in the surface's effects"
    );
    Ok(())
}

/// WHEN a slash immediate turn streams with no queue entry, THE
/// `handle_escape` reducer SHALL arm on the first Esc press and cancel on the
/// second.
#[test]
fn test_interaction_abort_cancel_queue_less_slash_immediate_arms_and_cancels() -> Result<()> {
    // -- Setup & Fixtures: an immediate submission the loop dispatches with no
    // queue entry, streamed into a busy surface.
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    state.enqueue_immediate_submission("/compact".to_string());
    let _ = state.take_pending_events(&cancel_controller);
    fold_record(&mut state, inference_started());
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.input_locked, "a live turn holds the input lock");
    assert!(
        state.prompt_items().is_empty(),
        "the immediate never enters the queue"
    );

    // -- Exec: one press to arm, one to confirm.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(
        !cancel_controller.is_cancel_requested(),
        "the arm does not cancel"
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the second press cancels.
    assert!(changed);
    assert!(
        cancel_controller.is_cancel_requested(),
        "the confirm press cancels the slash immediate's turn"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    Ok(())
}

/// WHEN a prompt-queue turn runs, THE escape coupling SHALL CONTINUE TO arm on
/// the first Esc press (the arm_confirm.rs contract) and cancel on the second,
/// and the confirm's `cancel_and_restore_pending_to_input` (state/lifecycle.rs:
/// 251-277) SHALL restore `input.restored_input_text` to the submitted text.
#[test]
fn test_interaction_abort_cancel_queue_less_queue_cycle_arms_confirms_and_restores_input()
-> Result<()> {
    // -- Setup & Fixtures: a surface submission queued through the enter
    // boundary, then the dispatched turn opened the inference cycle.
    let (mut state, cancel_controller) = busy_state_with_controller();
    fold_record(&mut state, inference_started());
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(
        state.pending_prompt_count(),
        1,
        "the submission still queues"
    );

    // -- Exec: one press to arm, one to confirm.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the arm, exactly the arm_confirm.rs contract.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(
        !cancel_controller.is_cancel_requested(),
        "the arm presses nothing: the confirm press owns the cancel"
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the confirm cancels, drains the queue, and the queue's text
    // returns to the input for the coordinator's pickup.
    assert!(changed);
    assert!(
        cancel_controller.is_cancel_requested(),
        "the confirm press flags the cancel"
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(
        state.pending_prompt_count(),
        0,
        "the queue cancel drains the queue"
    );
    assert_eq!(
        state.input.restored_input_text,
        Some("w".to_string()),
        "the queue cancel restores the submitted text to the input"
    );
    let events = state.take_pending_events(&cancel_controller);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::CancelRequested)),
        "the confirmed cancel must reach the loop as a CancelRequested event"
    );
    Ok(())
}

/// WHEN the confirm runs on a queue-less busy turn, THE
/// `cancel_and_restore_pending_to_input` call SHALL leave
/// `input.restored_input_text` `None`: the queue holds nothing to drain, so
/// the coordinator's pickup performs no textarea write.
#[test]
fn test_interaction_abort_cancel_queue_less_confirm_restores_no_input() -> Result<()> {
    // -- Setup & Fixtures: a peer turn armed by one Esc.
    let (mut state, cancel_controller) = queue_less_peer_turn();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the confirm ran, and nothing returns to the input.
    assert!(changed);
    assert!(
        cancel_controller.is_cancel_requested(),
        "the confirm press flags the cancel"
    );
    assert!(
        state.input.restored_input_text.is_none(),
        "a queue-less cancel restores nothing: the queue held no submitted text"
    );
    Ok(())
}

// -- Test Support

/// The peer-turn fixture: the row renders, the loop dispatches, and the queue
/// never sees the prompt, so the surface sits busy with the input locked while
/// its queue is empty.
fn queue_less_peer_turn() -> (AppState, CancelController) {
    let mut state = AppState::default();
    fold_record(
        &mut state,
        peer_prompt_record("q", None, None, None, "task-1"),
    );
    fold_record(&mut state, inference_started());
    (state, CancelController::default())
}

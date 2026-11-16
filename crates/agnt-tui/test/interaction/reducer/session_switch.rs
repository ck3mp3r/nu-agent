//! The session switch's display settle.
//!
//! `phase`, the input lock, an open permission card, and the compaction display
//! belong to the surface, not to a session, so the session that left can leave
//! them set. The switch settles them, and the replayed log's own turn boundaries
//! end every turn it closed at rest — the surface needs no live turn to leave the
//! spinner behind.

use super::*;

use crate::state::StatusMessageKind;
use crate::test_support::{
    compaction_started, inference_stopped_with_cause, prompt_record, session_agent_switched,
    session_model_switched, session_started_new, session_switched,
};
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::protocol::events::{CompactionSource, StopCause};

/// A switch into a session whose history holds an answered prompt lands at
/// rest: the replayed completion call takes the lock exactly as a live one
/// does, and the log's own turn boundary releases it.
#[test]
fn test_interaction_reducer_session_switch_replayed_boundary_releases_the_lock() -> Result<()> {
    // -- Setup & Fixtures: an idle surface switches into a session it has not
    // folded yet.
    let mut state = AppState::default();
    reduce_ui(&mut state, session_switched());

    // -- Exec: the replayed log of one answered prompt.
    reduce_ui(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );
    reduce_ui(&mut state, inference_started());
    let locked_by_the_replayed_call = state.input_locked;
    let changed = reduce_ui(&mut state, turn_completed(0));

    // -- Check: the replayed call took the lock and the replayed boundary is
    // what gave it back, so the surface is at rest with no live turn.
    assert!(
        locked_by_the_replayed_call,
        "a replayed completion call takes the lock exactly as a live one does"
    );
    assert!(
        changed,
        "the replayed turn boundary is a fold the surface handles"
    );
    assert_eq!(state.phase, UiPhase::Idle, "the replay ends at rest");
    assert!(
        !state.input_locked,
        "no release-less replay may strand the lock"
    );
    assert!(!state.is_active_cycle(), "no cycle survives the replay");
    assert_reducer_invariants(&state);
    Ok(())
}

/// The lock the previous session left is settled by the switch, so the session
/// that arrived starts at rest.
#[test]
fn test_interaction_reducer_session_switch_settles_a_stale_busy_lock() -> Result<()> {
    // -- Setup & Fixtures: the strand shape a release-less replay leaves — a
    // busy phase and a held lock with no prompt cycle behind them.
    let mut state = AppState {
        phase: UiPhase::Busy,
        input_locked: true,
        ..AppState::default()
    };
    assert!(!state.is_active_cycle(), "the fixture holds no cycle");

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the state that belonged to the session that left is gone.
    assert!(changed, "a session switch is a fold the surface handles");
    assert_eq!(state.phase, UiPhase::Idle, "the new session starts idle");
    assert!(!state.input_locked, "the lock does not cross the boundary");
    assert!(!state.is_active_cycle());
    assert_reducer_invariants(&state);
    Ok(())
}

/// A switch that folds while a live turn owns the surface leaves that turn
/// alone: its phase, its lock, and its place in the queue survive, and its
/// spine step still settles it afterwards.
#[test]
fn test_interaction_reducer_session_switch_leaves_a_live_turn_alone() -> Result<()> {
    // -- Setup & Fixtures: a running turn the surface locked.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(&mut state, inference_started());
    let active = active_prompt_id(&state);
    assert!(active.is_some(), "the fixture runs an active prompt");
    assert_eq!(state.phase, UiPhase::Busy);

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the live turn keeps the surface.
    assert!(changed, "a session switch is a fold the surface handles");
    assert_eq!(state.phase, UiPhase::Busy, "the live turn stays busy");
    assert!(state.input_locked, "the live turn keeps its lock");
    assert!(state.is_active_cycle());
    assert_eq!(
        active_prompt_id(&state),
        active,
        "the live turn keeps its active prompt"
    );
    assert_reducer_invariants(&state);

    // -- Exec: the live turn ends on the spine.
    fold_turn_completed(&mut state);

    // -- Check: the live turn settles exactly as it would without the switch.
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert_reducer_invariants(&state);
    Ok(())
}

/// The switch's stale-state settle is the switch's alone: the ordinary live
/// vocabulary around it moves no phase and no lock.
#[test]
fn test_interaction_reducer_session_switch_siblings_settle_nothing() -> Result<()> {
    // -- Setup & Fixtures: a busy surface the siblings must leave alone.
    let mut state = AppState {
        phase: UiPhase::Busy,
        input_locked: true,
        ..AppState::default()
    };

    // -- Exec
    let agent_switched = reduce_ui(&mut state, session_agent_switched());
    let model_switched = reduce_ui(&mut state, session_model_switched());

    // -- Check: a sibling switch is not a session boundary.
    assert!(!agent_switched, "an agent switch folds nothing");
    assert!(!model_switched, "a model switch folds nothing");
    assert_eq!(state.phase, UiPhase::Busy, "the turn keeps its phase");
    assert!(state.input_locked, "the turn keeps its lock");
    Ok(())
}

/// The latch, not the record, is what tells the fold whose boundary it is: a
/// `TurnRecord::Completed` that folds after the first live turn started is the
/// lossy ring's display-only record again.
#[test]
fn test_interaction_reducer_session_switch_closed_latch_boundary_is_display_only() -> Result<()> {
    // -- Setup & Fixtures: a switch opened the replay, and the first live turn
    // after it started.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(&mut state, session_switched());
    let live_start = LifecycleEvent::TurnStarted {
        prompt: "run".to_string(),
        task_id: None,
    };
    reduce(&mut state, RenderInput::Lifecycle(&live_start), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.input_locked);

    // -- Exec
    let changed = reduce_ui(&mut state, turn_completed(0));

    // -- Check: the closed latch makes the ring's boundary display-only again.
    assert!(!changed, "a live turn's ring boundary changes nothing");
    assert_eq!(state.phase, UiPhase::Busy, "the live turn keeps its phase");
    assert!(state.input_locked, "the live turn keeps its lock");
    assert_reducer_invariants(&state);
    Ok(())
}

/// A card the session that left was showing does not cross the boundary: with
/// no cycle behind it the card answers a request that left with its session,
/// and every key the arriving surface types is swallowed by a card that is not
/// its own.
#[test]
fn test_interaction_reducer_session_switch_settles_the_departed_card() -> Result<()> {
    // -- Setup & Fixtures: a card on a surface no cycle owns.
    let mut state = AppState::default();
    open_permission_card(&mut state);
    assert!(state.permission.has_prompt(), "the fixture shows a card");
    assert!(!state.is_active_cycle(), "the fixture runs no cycle");

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the departed session's card is gone.
    assert!(changed, "a session switch is a fold the surface handles");
    assert!(
        !state.permission.has_prompt(),
        "the departed session's card does not cross the boundary"
    );
    assert_reducer_invariants(&state);
    Ok(())
}

/// The compaction display the session that left was showing does not cross:
/// the block list describes rows the switch's transcript clear dropped, so the
/// arriving surface must not hold a running sweep.
#[test]
fn test_interaction_reducer_session_switch_settles_the_departed_compaction() -> Result<()> {
    // -- Setup & Fixtures: the departed session was compacting at rest.
    let mut state = AppState::default();
    reduce_ui(&mut state, compaction_started(CompactionSource::Auto));
    assert!(state.compaction.in_progress(), "the fixture is compacting");

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the arriving surface shows no compaction of its own.
    assert!(changed, "a session switch is a fold the surface handles");
    assert!(
        !state.compaction.in_progress(),
        "the departed session's compaction does not cross the boundary"
    );
    assert_reducer_invariants(&state);
    Ok(())
}

/// A live cycle still owns the surface, so the switch keeps its display state
/// exactly as it keeps the phase and the lock: the card is the running turn's
/// own, and the compaction runs under that turn.
#[test]
fn test_interaction_reducer_session_switch_leaves_a_live_turn_s_card_and_compaction() -> Result<()>
{
    // -- Setup & Fixtures: a running turn with a card up and a compaction
    // running beneath it.
    let mut state = busy_state_with_clean_transcript();
    open_permission_card(&mut state);
    reduce_ui(&mut state, compaction_started(CompactionSource::Auto));
    assert!(state.permission.has_prompt(), "the fixture shows a card");
    assert!(state.compaction.in_progress(), "the fixture is compacting");

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the live turn keeps every surface display it held.
    assert!(changed, "a session switch is a fold the surface handles");
    assert!(
        state.permission.has_prompt(),
        "the live turn's card is the live turn's"
    );
    assert!(
        state.compaction.in_progress(),
        "the live turn's compaction keeps running"
    );
    assert_eq!(state.phase, UiPhase::Busy, "the live turn stays busy");
    assert!(state.input_locked, "the live turn keeps its lock");
    assert_reducer_invariants(&state);
    Ok(())
}

/// A Notice has no TTL: nothing retires it but the next turn start, so a
/// surface no cycle owns must not carry the departed session's marker across
/// the boundary — a switch into a session with an empty history never starts the
/// turn that would clear it.
#[test]
fn test_interaction_reducer_session_switch_settles_the_departed_notice() -> Result<()> {
    // -- Setup & Fixtures: the cancel marker the departing session left.
    let mut state = AppState::default();
    reduce_ui(
        &mut state,
        inference_stopped_with_cause(StopCause::UserCancelled, "stop requested"),
    );
    assert_eq!(
        state.status.message.kind(),
        StatusMessageKind::Notice,
        "the fixture shows the marker the clock never retires"
    );
    assert!(!state.is_active_cycle(), "the fixture runs no cycle");

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the arriving surface starts with no status line of its own.
    assert!(changed, "a session switch is a fold the surface handles");
    assert!(
        state.status.message.status_line().is_empty(),
        "the departed session's marker does not cross the boundary"
    );
    assert_eq!(state.status.message.kind(), StatusMessageKind::Neutral);
    assert_reducer_invariants(&state);
    Ok(())
}

/// `/new` is the switch's twin: a fresh session is a new surface, so the display
/// the session that ended left behind must not cross into it either. Each piece
/// is the surface's own, not a session's.
#[test]
fn test_interaction_reducer_session_started_settles_the_departed_display() -> Result<()> {
    // -- Setup & Fixtures: the state the session that ended left, on a surface
    // no cycle owns.
    let mut state = AppState {
        phase: UiPhase::Busy,
        input_locked: true,
        ..AppState::default()
    };
    open_permission_card(&mut state);
    reduce_ui(&mut state, compaction_started(CompactionSource::Auto));
    reduce_ui(
        &mut state,
        inference_stopped_with_cause(StopCause::UserCancelled, "stop requested"),
    );
    assert!(state.permission.has_prompt(), "the fixture shows a card");
    assert!(state.compaction.in_progress(), "the fixture is compacting");

    // -- Exec
    let changed = reduce_ui(&mut state, session_started_new());

    // -- Check: the fresh session starts at rest, holding nothing of the one
    // that ended.
    assert!(
        changed,
        "a fresh session start is a fold the surface handles"
    );
    assert_eq!(state.phase, UiPhase::Idle, "the fresh session starts idle");
    assert!(!state.input_locked, "the lock does not cross the boundary");
    assert!(
        !state.permission.has_prompt(),
        "the departed session's card does not cross the boundary"
    );
    assert!(
        !state.compaction.in_progress(),
        "the departed session's compaction does not cross the boundary"
    );
    assert!(
        state.status.message.status_line().is_empty(),
        "the departed session's marker does not cross the boundary"
    );
    assert_eq!(
        state.transcript.len(),
        1,
        "the fresh transcript holds exactly the startup banner"
    );
    assert_reducer_invariants(&state);
    Ok(())
}

/// A live turn owns the surface, so the switch keeps its status line exactly as
/// it keeps the phase and the lock: the marker is the running turn's own.
#[test]
fn test_interaction_reducer_session_switch_leaves_a_live_turn_s_notice() -> Result<()> {
    // -- Setup & Fixtures: a running turn with its cancel marker up.
    let mut state = busy_state_with_clean_transcript();
    reduce_ui(
        &mut state,
        inference_stopped_with_cause(StopCause::UserCancelled, "stop requested"),
    );
    assert_eq!(state.status.message.kind(), StatusMessageKind::Notice);

    // -- Exec
    let changed = reduce_ui(&mut state, session_switched());

    // -- Check: the live turn keeps every display it held.
    assert!(changed, "a session switch is a fold the surface handles");
    assert_eq!(
        state.status.message.status_line(),
        "stop requested",
        "the live turn's marker is the live turn's"
    );
    assert_eq!(state.phase, UiPhase::Busy, "the live turn stays busy");
    assert!(state.input_locked, "the live turn keeps its lock");
    assert_reducer_invariants(&state);
    Ok(())
}

/// A replayed log's usage is history, not a call the surface made: switching
/// into the same history repeatedly re-places the token figure and leaves the
/// dollars' accumulated value where the live calls left it.
#[test]
fn test_interaction_reducer_session_switch_repeated_replay_holds_the_figures() -> Result<()> {
    // -- Setup & Fixtures: one live priced cycle, the shape a real call leaves.
    let mut state = AppState::default();
    reduce_ui(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );
    reduce_ui(&mut state, inference_started());
    reduce_ui(
        &mut state,
        inference_completed(40, 20, Some(60), Some(0.002), true),
    );
    reduce_ui(&mut state, turn_completed(0));
    let mut displayed = vec![state.status.tokens.latest_total_tokens];
    let mut labels = vec![state.status.tokens.session_cost.label()];

    for _ in 0..2 {
        // -- Exec: the switch, then the replayed log of the same answered prompt.
        reduce_ui(&mut state, session_switched());
        reduce_ui(
            &mut state,
            prompt_record("hi", Some(1_700_000_000_000), None, None),
        );
        reduce_ui(&mut state, inference_started());
        reduce_ui(
            &mut state,
            inference_completed(40, 20, Some(60), None, false),
        );
        reduce_ui(&mut state, turn_completed(0));
        displayed.push(state.status.tokens.latest_total_tokens);
        labels.push(state.status.tokens.session_cost.label());
    }

    // -- Check: the token figure re-places to the history's own total on every
    // switch, and no repeat adds its usage again.
    assert_eq!(
        displayed,
        vec![Some(60), Some(60), Some(60)],
        "each switch places the history's figure, and none accumulates it"
    );

    // -- Check: the dollars hold their accumulated value. A replayed call is
    // unpriced, so the label gains the lower-bound marker; it adds no money, and
    // the repeat adds nothing more.
    assert_eq!(
        labels[0].as_deref(),
        Some("$0.002"),
        "the live priced call is the only money the figure sees"
    );
    assert_eq!(
        labels[1].as_deref(),
        Some("$0.002+"),
        "the replay adds the unpriced marker and no money"
    );
    assert_eq!(
        labels[2].as_deref(),
        Some("$0.002+"),
        "a repeat switch adds nothing to the dollars"
    );
    Ok(())
}

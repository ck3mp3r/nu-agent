use super::*;

use crate::interaction::cancel::CancelController;
use crate::test_support::{fold_record, peer_prompt_record, prompt_record};
use agnt_runtime::orchestrator::events::OrchestratorEvent;

/// WHEN two external tasks' prompts arrive back to back, THE surface fold SHALL
/// render one row per prompt and SHALL NOT submit either one.
///
/// The loop's control arm is the sole external intake: it dispatches the turn
/// itself. A surface that ingests the prompt and submits it again runs the
/// prompt a second time, unattributed, behind the running turn.
#[test]
fn stacked_external_prompts_render_without_echoing_a_submit() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let controller = CancelController::default();
    let before = state.transcript.len();

    // -- Exec: two arrivals back to back, as the wire delivers them.
    enqueue_external(&mut state, "task-a", "prompt a");
    enqueue_external(&mut state, "task-b", "prompt b");

    // -- Check: both prompts render, in arrival order.
    assert_eq!(user_rows(&state), vec!["prompt a", "prompt b"]);
    assert_eq!(state.transcript.len(), before + 2);

    // -- Exec: the render loop drains the surface's outbound effects.
    let events = state.take_pending_events(&controller);

    // -- Check: no prompt is submitted and none is queued.
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::PromptSubmitted { .. })),
        "the surface must not echo an external prompt back as a submit"
    );
    assert!(
        state.prompt_items().is_empty(),
        "the surface must not queue an external prompt for submission"
    );
    Ok(())
}

/// WHEN a prompt record without a task attribution folds live, THE surface SHALL
/// draw the row and SHALL NOT reach the submit queue.
///
/// That record shape is the replay-shaped one. The fold has one prompt arm for
/// every row, so no publisher of a prompt record can make the surface re-execute
/// a prompt as a new user turn behind the loop's own dispatch.
#[test]
fn prompt_record_without_a_task_id_places_a_row_without_submitting() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let controller = CancelController::default();

    // -- Exec
    fold_record(
        &mut state,
        prompt_record("unattributed question", None, None, None),
    );

    // -- Check: the row is drawn, nothing is queued, nothing is submitted.
    assert_eq!(user_rows(&state), vec!["unattributed question"]);
    assert!(
        state.prompt_items().is_empty(),
        "a record row never enters the prompt queue"
    );
    assert!(
        !state
            .take_pending_events(&controller)
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::PromptSubmitted { .. })),
        "a record row must never echo back as a submit"
    );
    Ok(())
}

/// WHEN the fold receives a peer prompt record whose stamps equal the stamps a
/// queued submission carried, THE rendered user row SHALL equal the queue path's
/// row field for field.
#[test]
fn peer_prompt_row_matches_the_queue_path_row() -> Result<()> {
    // -- Setup & Fixtures: one prompt and one set of stamps, as the boundary that
    // accepted it recorded them.
    let millis = Some(1_700_000_000_000i64);
    let model = Some("openai/gpt-4o".to_string());
    let agent = Some("bob".to_string());
    let mut peer = AppState::default();
    let mut queued = AppState::default();

    // -- Exec: the same prompt reaches the transcript by the two roads — the
    // record the loop publishes for a peer's arrival, and the surface's own
    // submit queue.
    fold_record(
        &mut peer,
        peer_prompt_record(
            "mailbox message",
            millis,
            model.clone(),
            agent.clone(),
            "task-a",
        ),
    );
    queued.enqueue_prompt_with_facts(
        "mailbox message".to_string(),
        PromptFacts {
            millis,
            model,
            agent,
        },
    );
    let _ = queued.take_pending_events(&CancelController::default());

    // -- Check: one row each, equal field for field.
    assert_eq!(
        user_messages(&peer),
        user_messages(&queued),
        "the peer prompt's row must be the row the queue path rendered"
    );
    Ok(())
}

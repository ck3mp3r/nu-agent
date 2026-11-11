//! Single-turn dispatch: exactly one turn, carrying the caller's attribution, and
//! no spine step published by the dispatch layer.

use super::*;

use agnt_runtime::conversation::turn::{TurnEnd, TurnFailure};

/// A runtime that records the turns it runs, publishes no lifecycle step, and
/// reports the turn end it is scripted with.
#[derive(Default)]
struct SingleTurnRuntime {
    prompts: Vec<String>,
    task_ids: Vec<Option<String>>,
    /// The end the next turn reports. `None` reports a completed turn.
    turn_end: Option<TurnEnd>,
}

impl agnt_runtime::protocol::contracts::CoreRuntime for SingleTurnRuntime {
    async fn execute_turn(
        &mut self,
        prompt: String,
        _context: Option<String>,
        span: Span,
        task_id: Option<String>,
    ) -> TurnEnd {
        self.prompts.push(prompt);
        self.task_ids.push(task_id);
        self.turn_end.clone().unwrap_or(TurnEnd::Completed {
            value: Value::nothing(span),
        })
    }
}

#[tokio::test]
async fn test_mode_run_single_turn_dispatches_exactly_one_unattributed_turn() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut turn_rx = bus.lifecycle().subscribe();
    let mut runtime = SingleTurnRuntime::default();

    // -- Exec
    agnt_runtime::orchestrator::run_single_turn(
        &mut runtime,
        None,
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    // -- Check: one dispatch, carrying no task attribution. The turn executor
    // owns the spine bracket, so the dispatch layer publishes none.
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    assert_eq!(
        runtime.task_ids,
        vec![None],
        "a user prompt's single turn dispatches exactly once, unattributed"
    );
    let mut published = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        published.push(event);
    }
    assert!(
        published.is_empty(),
        "the dispatch layer must publish no spine step; got: {published:?}"
    );
    Ok(())
}

/// WHEN the caller holds the id of the external task its turn runs for, THE
/// `run_single_turn` function SHALL hand that id to the turn, so the executor's
/// spine bracket carries the peer's attribution.
#[tokio::test]
async fn test_mode_run_single_turn_carries_the_caller_task_id() -> Result<()> {
    // -- Setup & Fixtures
    let mut runtime = SingleTurnRuntime::default();

    // -- Exec
    agnt_runtime::orchestrator::run_single_turn(
        &mut runtime,
        Some("peer-task-1".to_string()),
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    // -- Check
    assert_eq!(
        runtime.task_ids,
        vec![Some("peer-task-1".to_string())],
        "the caller's task id must reach the turn unchanged"
    );
    Ok(())
}

/// WHEN a one-shot turn ends cancelled, THE `run_single_turn` back-mapping SHALL
/// return the turn's response value as `Ok`, so the stderr and plugin paths keep
/// the behavior they had before the turn end was typed.
#[tokio::test]
async fn test_mode_run_single_turn_maps_a_cancelled_end_to_ok() -> Result<()> {
    // -- Setup & Fixtures
    let value = Value::string("stopped response", Span::unknown());
    let mut runtime = SingleTurnRuntime {
        turn_end: Some(TurnEnd::Cancelled {
            value: value.clone(),
        }),
        ..Default::default()
    };

    // -- Exec
    let result = agnt_runtime::orchestrator::run_single_turn(
        &mut runtime,
        None,
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await;

    // -- Check
    let returned = result.map_err(|e| format!("a cancelled turn must map to Ok: {e}"))?;
    assert_eq!(
        returned, value,
        "the cancelled turn's response value is the one-shot result"
    );
    Ok(())
}

/// WHEN a one-shot turn fails, THE `run_single_turn` back-mapping SHALL return
/// the failure's user-facing error.
#[tokio::test]
async fn test_mode_run_single_turn_maps_a_failed_end_to_err() -> Result<()> {
    // -- Setup & Fixtures
    let mut runtime = SingleTurnRuntime {
        turn_end: Some(TurnEnd::Failed(TurnFailure::from_error(
            nu_protocol::LabeledError::new("the provider refused the turn"),
        ))),
        ..Default::default()
    };

    // -- Exec
    let result = agnt_runtime::orchestrator::run_single_turn(
        &mut runtime,
        None,
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await;

    // -- Check
    let err = result.err().ok_or("a failed turn must map to Err")?;
    assert_eq!(err.msg, "the provider refused the turn");
    Ok(())
}

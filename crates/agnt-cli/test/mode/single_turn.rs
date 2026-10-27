//! Single-turn dispatch: exactly one unattributed `TurnEvent::Started`.

use super::*;

/// A runtime that records the prompts it runs and publishes no turn events.
#[derive(Default)]
struct SingleTurnRuntime {
    prompts: Vec<String>,
}

impl agnt_core::protocol::contracts::CoreRuntime for SingleTurnRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &agnt_core::bus::Bus,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> core::result::Result<Value, nu_protocol::LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(span))
    }
}

#[tokio::test]
async fn test_mode_run_single_turn_publishes_exactly_one_started() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut turn_rx = bus.turn().subscribe();
    let mut runtime = SingleTurnRuntime::default();

    // -- Exec
    agnt_core::orchestrator::run_single_turn(
        &mut runtime,
        &bus,
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    // -- Check
    let mut started = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        if let TurnEvent::Started { prompt, task_id } = event {
            started.push((prompt, task_id));
        }
    }
    assert_eq!(
        started,
        vec![("hello".to_string(), None)],
        "a single turn must publish exactly one unattributed TurnEvent::Started"
    );
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

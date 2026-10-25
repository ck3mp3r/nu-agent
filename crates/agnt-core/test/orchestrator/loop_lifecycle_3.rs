//! `TurnEvent::Started` emission for the interactive turn path.
//!
//! The turn-dispatch layer owns the emission: the worker router publishes one
//! `Started` per `ExecuteTurn`, carrying the prompt and the A2A task
//! attribution. The slash stage no longer publishes it.

use super::*;

/// The `TurnEvent::Started` events a receiver holds, as `(prompt, task_id)`.
fn started_turns(rx: &mut crate::bus::TurnRx) -> Vec<(String, Option<String>)> {
    let mut started = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let crate::bus::TurnEvent::Started { prompt, task_id } = event {
            started.push((prompt, task_id));
        }
    }
    started
}

#[tokio::test]
async fn interactive_loop_publishes_exactly_one_turn_started_per_turn() -> TResult {
    // -- Setup & Fixtures
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello"]).with_bus(bus.clone());
    let mut turn_rx = bus.turn().subscribe();
    let spawner = ui.make_event_spawner();

    // -- Exec
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    // -- Check
    assert_eq!(
        started_turns(&mut turn_rx),
        vec![("hello".to_string(), None)],
        "a turn must publish exactly one unattributed TurnEvent::Started"
    );
    Ok(())
}

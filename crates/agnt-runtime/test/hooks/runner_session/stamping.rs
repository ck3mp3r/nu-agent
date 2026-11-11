//! Session-id stamping: the runner tracks the id from `Started` and `Switched`.

use super::*;

#[tokio::test]
async fn test_hooks_runner_stamps_the_agent_context_on_every_envelope() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    publish_turn_completed(
        &bus,
        1,
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        },
        0,
    );
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    let envelope = envelope_json(&calls[0])?;
    assert_eq!(envelope["agent"], "reviewer", "the slug is stamped");
    assert_eq!(envelope["model"], "openai/gpt-4o", "the model is stamped");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_stamps_session_id_from_started() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
        session_id: "sess-1".to_string(),
        hydrated: false,
    });
    publish_turn_completed(
        &bus,
        1,
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        },
        0,
    );
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "only the turn event invokes the hook");
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_stamps_session_id_from_switched() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionSwitched {
        from: Some("sess-1".to_string()),
        to: "sess-2".to_string(),
    });
    publish_turn_completed(
        &bus,
        1,
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        },
        0,
    );
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-2",
        "Switched must stamp to_session_id"
    );
    Ok(())
}

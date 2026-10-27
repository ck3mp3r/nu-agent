//! Session-id stamping: the runner tracks the id from `Started` and `Switched`.

use super::*;

#[tokio::test]
async fn test_hooks_runner_stamps_the_agent_context_on_every_envelope() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Completed {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: false,
        })
        .await;
    let _ = bus
        .turn()
        .send(TurnEvent::Completed {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
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
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Switched {
            from_session_id: Some("sess-1".to_string()),
            to_session_id: "sess-2".to_string(),
        })
        .await;
    let _ = bus
        .turn()
        .send(TurnEvent::Completed {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
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

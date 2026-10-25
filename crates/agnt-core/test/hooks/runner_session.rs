//! Session event mapping, session-id stamping, and the observe-only guarantee.

use super::*;

#[tokio::test]
async fn test_hooks_runner_session_started_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::SessionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: true,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "session_started");
    let body = payload(&calls[0])?;
    assert_eq!(body["session_id"], "sess-1");
    assert_eq!(body["hydrated"], true);
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_session_ended_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::SessionEnded])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Ended {
            session_id: "sess-1".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "session_ended");
    assert_eq!(payload(&calls[0])?["session_id"], "sess-1");
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
        .send(TurnEvent::Completed { tool_calls: 1 })
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
        .send(TurnEvent::Completed { tool_calls: 1 })
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

#[tokio::test]
async fn test_hooks_runner_fires_again_for_a_repeated_started() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::SessionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    for _ in 0..2 {
        let _ = bus
            .session()
            .send(SessionEvent::Started {
                session_id: "sess-1".to_string(),
                hydrated: false,
            })
            .await;
    }
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "a repeated Started with no intervening Ended must fire again; got: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_skips_agent_class_only_hooks() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "notify",
        &[HookEvent::AgentStarted, HookEvent::AgentShutdown],
    )];

    // -- Exec
    let handles = spawn(&bus, &hooks, &process);
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "an agent-class-only hook must not run on the session runner"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[HookEvent::TurnStarted, HookEvent::SessionStarted],
    )];
    let mut compaction_rx = bus.compaction().subscribe();
    let mut cancel_rx = bus.cancel().subscribe();
    let mut external_rx = bus.external().subscribe();
    let mut ui_state_rx = bus.ui_state().subscribe();
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: None,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert_eq!(process.calls().len(), 1, "the hook must have run");
    assert!(
        compaction_rx.try_recv().is_err(),
        "the runner must not publish a compaction event"
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "the runner must not publish a cancel event"
    );
    assert!(
        external_rx.try_recv().is_err(),
        "the runner must not publish an external event"
    );
    assert!(
        ui_state_rx.try_recv().is_err(),
        "the runner must not publish a ui-state event"
    );
    Ok(())
}

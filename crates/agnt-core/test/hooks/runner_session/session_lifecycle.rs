//! Session-class lifecycle events: `session_started` and `session_ended`.

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
            input_tokens: 30,
            output_tokens: 12,
            total_tokens: Some(42),
            cost: Some(0.3),
            cost_complete: false,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "session_ended");
    let body = payload(&calls[0])?;
    assert_eq!(body["session_id"], "sess-1");
    assert_eq!(body["input_tokens"], 30);
    assert_eq!(body["output_tokens"], 12);
    assert_eq!(body["total_tokens"], 42);
    assert_eq!(body["cost"], 0.3);
    assert_eq!(body["cost_complete"], false);
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

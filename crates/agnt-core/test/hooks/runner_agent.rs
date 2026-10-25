//! Agent-class hook runner: process-wide events, no session tracking.

use super::*;

#[tokio::test]
async fn test_hooks_agent_runner_started_invokes_with_mode() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("notify", &[HookEvent::AgentStarted])];
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "tui".to_string(),
        })
        .await;
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "one agent event must invoke the hook once");
    assert_eq!(calls[0].event, "agent_started");
    assert_eq!(calls[0].script, PathBuf::from("/hooks/notify.nu"));
    assert_eq!(calls[0].cwd, session_cwd());
    assert_eq!(payload(&calls[0])?["mode"], "tui");
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "",
        "agent envelopes always carry an empty session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_shutdown_invokes_with_reason() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("notify", &[HookEvent::AgentShutdown])];
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    let _ = agent_tx
        .send(AgentEvent::Shutdown {
            reason: "error".to_string(),
        })
        .await;
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "agent_shutdown");
    assert_eq!(payload(&calls[0])?["reason"], "error");
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "",
        "agent envelopes always carry an empty session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_skips_hooks_without_agent_events() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];

    // -- Exec
    let handles = spawn_agent(&agent_tx, &hooks, &process);
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "tui".to_string(),
        })
        .await;
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a hook with no agent-class events must not run on the agent runner"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_ignores_bus_session_activity() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("notify", &[HookEvent::AgentStarted])];
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: false,
        })
        .await;
    let _ = bus
        .session()
        .send(SessionEvent::Switched {
            from_session_id: Some("sess-1".to_string()),
            to_session_id: "sess-2".to_string(),
        })
        .await;
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "acp".to_string(),
        })
        .await;
    drop(bus);
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "",
        "session activity on a coexisting bus must not leak into agent envelopes"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_forwards_every_event() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("notify", &[HookEvent::AgentStarted])];
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    for _ in 0..2 {
        let _ = agent_tx
            .send(AgentEvent::Started {
                mode: "tui".to_string(),
            })
            .await;
    }
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    assert_eq!(
        process.calls().len(),
        2,
        "the agent runner forwards every event without dedup"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_and_session_runner_each_invoke_a_both_class_hook() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[HookEvent::TurnCompleted, HookEvent::AgentStarted],
    )];
    let session_handles = spawn(&bus, &hooks, &process);
    let agent_handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Completed { tool_calls: 1 })
        .await;
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "tui".to_string(),
        })
        .await;
    drop(bus);
    drop(agent_tx);
    finish(session_handles).await?;
    finish(agent_handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "each runner must invoke the hook for its own class; got: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| call.event == "turn_completed"),
        "the session runner must invoke the turn event; got: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| call.event == "agent_started"),
        "the agent runner must invoke the agent event; got: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_continues_after_a_failure() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::failing();
    let hooks = vec![hook("notify", &[HookEvent::AgentStarted])];
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    for _ in 0..2 {
        let _ = agent_tx
            .send(AgentEvent::Started {
                mode: "tui".to_string(),
            })
            .await;
    }
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    assert_eq!(
        process.calls().len(),
        2,
        "a failed agent hook must not stop the loop"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_agent_runner_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let agent_tx = AgentTx::new("agent", 16);
    let process = MockProcess::default();
    let hooks = vec![hook("notify", &[HookEvent::AgentStarted])];
    let mut compaction_rx = bus.compaction().subscribe();
    let mut cancel_rx = bus.cancel().subscribe();
    let mut external_rx = bus.external().subscribe();
    let mut ui_state_rx = bus.ui_state().subscribe();
    let mut session_rx = bus.session().subscribe();
    let handles = spawn_agent(&agent_tx, &hooks, &process);

    // -- Exec
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "tui".to_string(),
        })
        .await;
    drop(agent_tx);
    finish(handles).await?;

    // -- Check
    assert_eq!(process.calls().len(), 1, "the hook must have run");
    assert!(
        compaction_rx.try_recv().is_err(),
        "the agent runner must not publish a compaction event"
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "the agent runner must not publish a cancel event"
    );
    assert!(
        external_rx.try_recv().is_err(),
        "the agent runner must not publish an external event"
    );
    assert!(
        ui_state_rx.try_recv().is_err(),
        "the agent runner must not publish a ui-state event"
    );
    assert!(
        session_rx.try_recv().is_err(),
        "the agent runner must not publish a session event"
    );
    Ok(())
}

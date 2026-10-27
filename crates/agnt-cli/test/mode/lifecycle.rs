//! The process-wide agent lifecycle: the Started/Shutdown pair and its drain.

use super::support::*;
use super::*;

#[tokio::test]
async fn test_mode_agent_lifecycle_start_publishes_started_with_mode() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook(
        "notify",
        &[HookEvent::AgentStarted, HookEvent::AgentShutdown],
    )];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    )
    .await;
    let Ok(event) = agent_rx.try_recv() else {
        return Err("the agent channel must carry AgentEvent::Started".into());
    };
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    match event {
        AgentEvent::Started { mode } => assert_eq!(mode, "tui"),
        other => return Err(format!("expected AgentEvent::Started, got {other:?}").into()),
    }
    assert_eq!(drained, 1, "one agent-class hook must spawn one runner");
    let calls = process.calls();
    assert_eq!(calls.len(), 2, "agent_started then agent_shutdown");
    assert_eq!(calls[0].0, "agent_started");
    assert_eq!(calls[1].0, "agent_shutdown");
    assert!(
        calls[0].1.contains("\"agent\":\"reviewer\""),
        "the envelope must carry the persona slug; got: {}",
        calls[0].1
    );
    assert!(
        calls[0].1.contains("\"model\":\"openai/gpt-4o\""),
        "the envelope must carry the active model; got: {}",
        calls[0].1
    );
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_shutdown_publishes_reason() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook("notify", &[HookEvent::AgentShutdown])];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "stderr",
        &hooks,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("error").await;
    lifecycle.drain().await;

    // -- Check
    let mut reasons = Vec::new();
    while let Ok(event) = agent_rx.try_recv() {
        if let AgentEvent::Shutdown { reason } = event {
            reasons.push(reason);
        }
    }
    assert_eq!(
        reasons,
        vec!["error".to_string()],
        "the shutdown reason must reach the agent channel"
    );
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "only agent_shutdown is subscribed");
    assert_eq!(calls[0].0, "agent_shutdown");
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_without_agent_hooks_spawns_no_runner() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook("audit", &[HookEvent::TurnCompleted])];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    assert_eq!(
        drained, 0,
        "a hook with no agent-class event must spawn no agent runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_spawns_one_runner_per_agent_hook() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = RecordingProcess::default();
    let hooks = vec![
        resolved_hook("one", &[HookEvent::AgentStarted]),
        resolved_hook("two", &[HookEvent::AgentStarted, HookEvent::AgentShutdown]),
    ];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    assert_eq!(drained, 2, "one runner per agent-class hook");
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        3,
        "hook one: agent_started; hook two: agent_started + agent_shutdown"
    );
    Ok(())
}

#[test]
fn test_mode_shutdown_reason_maps_ok_and_err() -> Result<()> {
    // -- Setup & Fixtures
    let ok: crate::error::Result<Value> = Ok(Value::nothing(Span::unknown()));
    let err: crate::error::Result<Value> = Err(crate::error::Error::Custom("nope".to_string()));

    // -- Exec & Check
    assert_eq!(shutdown_reason(&ok, "completed"), "completed");
    assert_eq!(shutdown_reason(&ok, "shutdown"), "shutdown");
    assert_eq!(shutdown_reason(&err, "completed"), "error");
    assert_eq!(shutdown_reason(&err, "shutdown"), "error");
    Ok(())
}

#[tokio::test]
async fn test_mode_run_with_agent_lifecycle_shuts_down_on_early_body_error() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook(
        "notify",
        &[HookEvent::AgentStarted, HookEvent::AgentShutdown],
    )];
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    )
    .await;

    // -- Exec
    let result: crate::error::Result<Value> =
        run_with_agent_lifecycle(lifecycle, "completed", || async {
            Err(crate::error::Error::Custom(
                "renderer init failed".to_string(),
            ))
        })
        .await;

    // -- Check
    assert!(result.is_err(), "the body error must reach the caller");
    let mut reasons = Vec::new();
    while let Ok(event) = agent_rx.try_recv() {
        if let AgentEvent::Shutdown { reason } = event {
            reasons.push(reason);
        }
    }
    assert_eq!(
        reasons,
        vec!["error".to_string()],
        "an early body failure must still publish AgentEvent::Shutdown"
    );
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "the hook must observe agent_started and agent_shutdown"
    );
    Ok(())
}

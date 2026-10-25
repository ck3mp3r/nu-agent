use super::*;

// ---------------------------------------------------------------------------
// Turn-start emission
// ---------------------------------------------------------------------------

/// Drain every event currently buffered on the turn channel.
fn take_turn_events(rx: &mut TurnRx) -> Vec<TurnEvent> {
    let mut events = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(event) => events.push(event),
            Err(agnt_core::bus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    events
}

#[tokio::test]
async fn test_session_manager_prompt_publishes_turn_started() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();
    // Subscribe before the turn runs: a broadcast channel does not buffer for
    // later subscribers.
    let mut turn_rx = manager
        .get_session(&session_id)
        .ok_or("session must exist")?
        .bus()
        .turn()
        .subscribe();

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await?;

    // -- Check
    let started: Vec<(String, Option<String>)> = take_turn_events(&mut turn_rx)
        .into_iter()
        .filter_map(|event| match event {
            TurnEvent::Started { prompt, task_id } => Some((prompt, task_id)),
            _ => None,
        })
        .collect();
    assert_eq!(
        started,
        vec![("hello".to_string(), None)],
        "one ACP prompt turn must publish exactly one Started with the prompt and no attribution"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_prompt_publishes_turn_started_before_completed() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();
    let mut turn_rx = manager
        .get_session(&session_id)
        .ok_or("session must exist")?
        .bus()
        .turn()
        .subscribe();

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await?;

    // -- Check
    let events = take_turn_events(&mut turn_rx);
    let started_at = events
        .iter()
        .position(|event| matches!(event, TurnEvent::Started { .. }))
        .ok_or(format!("the turn must publish Started; got: {events:?}"))?;
    let completed_at = events
        .iter()
        .position(|event| matches!(event, TurnEvent::Completed { .. }))
        .ok_or(format!("the turn must publish Completed; got: {events:?}"))?;
    assert!(
        started_at < completed_at,
        "Started must precede Completed; got: {events:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Concurrent prompt + cancel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_cancel_during_prompt_returns_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // Park the turn on the session's runtime lock: the prompt is then in flight
    // but has not started its turn, which is the state a real turn spends its
    // whole life in.
    let runtime = Arc::clone(
        manager
            .get_session(&session_id)
            .ok_or("session must exist")?
            .runtime(),
    );
    let guard = runtime.lock().await;

    // -- Exec
    let prompt = manager.prompt(&session_id, "hello".to_string(), &sink);
    tokio::pin!(prompt);
    let parked = tokio::time::timeout(Duration::from_millis(50), &mut prompt).await;
    let cancel = manager.cancel(&session_id).await;
    drop(guard);
    let stop_reason = prompt.await?;

    // -- Check
    assert!(
        parked.is_err(),
        "the prompt must park on the runtime lock while the turn is in flight"
    );
    assert!(
        cancel.is_ok(),
        "cancel must complete while the turn holds the runtime lock"
    );
    assert_eq!(stop_reason, StopReason::Cancelled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt permission bridge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_prompt_bridge_shares_runtime_pending_map() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // Register a decision in the runtime's pending map. The bridge that
    // `prompt()` builds must drain this same map when the turn ends.
    let pending = {
        let session = manager
            .get_session(&session_id)
            .ok_or("session must exist")?;
        let runtime = session.runtime().lock().await;
        runtime
            .interactive_pending_arc()
            .ok_or("the ACP runtime must install the interactive pending map")?
    };
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("test-permission");
    pending
        .lock()
        .map_err(|_| "pending map must not be poisoned")?
        .insert("req-1".to_string(), tx);

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await;
    let decision = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .map_err(|_| "the bridge must drain the runtime pending map")??;

    // -- Check
    assert_eq!(
        decision,
        PermissionDecision::Deny,
        "an unresolved request must be denied when the turn ends"
    );
    Ok(())
}

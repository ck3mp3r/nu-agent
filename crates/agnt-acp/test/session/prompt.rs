use super::*;

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

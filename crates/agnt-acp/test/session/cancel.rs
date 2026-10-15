use super::*;

// ---------------------------------------------------------------------------
// cancel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_cancel_signals_the_session_bus() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let bus = manager
        .get_session(&session_id)
        .ok_or("session must exist")?
        .bus()
        .clone();
    let mut cancel_rx = bus.cancel().subscribe();

    // -- Exec
    manager.cancel(&session_id).await?;
    let event = cancel_rx.recv().await?;

    // -- Check
    assert!(
        matches!(event, CancelEvent::Requested),
        "cancel must publish CancelEvent::Requested on the session bus"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_cancel_unknown_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.cancel("no-such-session").await;

    // -- Check
    let err = result
        .err()
        .ok_or("cancelling an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

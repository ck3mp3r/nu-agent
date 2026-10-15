use super::*;

// ---------------------------------------------------------------------------
// load_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_load_session_replays_history() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::default();

    // -- Exec
    let response = manager.load_session("sess-1", cwd, &sink).await?;

    // -- Check
    let updates = sink.taken();
    assert_eq!(
        updates.len(),
        2,
        "both stored messages must be replayed; got: {updates:?}"
    );
    assert!(
        matches!(updates[0], SessionUpdate::UserMessageChunk(_)),
        "the user turn must replay as user_message_chunk; got: {:?}",
        updates[0]
    );
    assert!(
        matches!(updates[1], SessionUpdate::AgentMessageChunk(_)),
        "the assistant turn must replay as agent_message_chunk; got: {:?}",
        updates[1]
    );
    assert_eq!(chunk_text(&updates[0]).as_deref(), Some("hello"));
    assert_eq!(chunk_text(&updates[1]).as_deref(), Some("hi there"));
    assert_eq!(response, LoadSessionResponse::new());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_registers_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::default();

    // -- Exec
    manager.load_session("sess-1", cwd.clone(), &sink).await?;
    let session = manager
        .get_session("sess-1")
        .ok_or("a loaded session must be live")?;

    // -- Check
    assert_eq!(session.id(), "sess-1");
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_missing_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, _store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let sink = RecordingSink::default();

    // -- Exec
    let result = manager.load_session("no-such-session", cwd, &sink).await;

    // -- Check
    let err = result.err().ok_or("loading a missing session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    assert!(
        sink.taken().is_empty(),
        "nothing must be replayed for a missing session"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_failing_sink_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::failing();

    // -- Exec
    let result = manager.load_session("sess-1", cwd, &sink).await;

    // -- Check
    let err = result
        .err()
        .ok_or("a dead transport must fail the replay")?;
    assert!(
        matches!(err, AcpError::Runtime(_)),
        "a dead transport must produce AcpError::Runtime, got: {err:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// resume_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_resume_session_returns_id() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;

    // -- Exec
    let session_id = manager.resume_session("sess-1", cwd.clone()).await?;
    let session = manager
        .get_session(&session_id)
        .ok_or("a resumed session must be live")?;

    // -- Check
    assert_eq!(session_id, "sess-1");
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_resume_session_missing_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, _store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let result = manager.resume_session("no-such-session", cwd).await;

    // -- Check
    let err = result.err().ok_or("resuming a missing session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    assert!(
        manager.get_session("no-such-session").is_none(),
        "a failed resume must not register a session"
    );
    Ok(())
}

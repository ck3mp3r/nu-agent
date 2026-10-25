use super::*;

// ---------------------------------------------------------------------------
// new_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_new_session_returns_non_empty_id() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    assert!(!session_id.is_empty(), "session id must not be empty");
    Ok(())
}

#[tokio::test]
async fn test_session_manager_new_session_registers_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd.clone(), Vec::new()).await?;
    let session = manager
        .get_session(&session_id)
        .ok_or("a new session must be retrievable")?;

    // -- Check
    assert_eq!(session.id(), session_id);
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_new_session_builder_error_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = AcpSessionManager::with_runtime_builder(
        rig_config(temp.path()),
        temp.path().join("config.toml"),
        tokio::runtime::Handle::current(),
        FailingRuntimeBuilder,
    );

    // -- Exec
    let result = manager
        .new_session(temp.path().to_path_buf(), Vec::new())
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("a failing runtime builder must fail session creation")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "the builder error must propagate, got: {err:?}"
    );
    assert!(
        manager.list_sessions().is_empty(),
        "a session that failed to build must not be registered"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// client filesystem state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_open_session_passes_client_fs_when_initialized() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_client_fs(temp.path()).await?;
    let (connection, _peer) = client_connection().await?;
    let capabilities = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let state = Arc::new(Mutex::new(Some(ClientFsState {
        connection,
        capabilities: capabilities.clone(),
    })));
    let manager = manager.with_client_fs(state);
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let passed = captured
        .first()
        .ok_or("the runtime builder must be called once")?
        .clone()
        .ok_or("open_session must pass Some(ClientFsState) when initialize stored one")?;
    assert_eq!(passed.capabilities, capabilities);
    Ok(())
}

#[tokio::test]
async fn test_session_manager_open_session_passes_none_client_fs_without_initialize() -> Result<()>
{
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_client_fs(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let passed = captured
        .first()
        .ok_or("the runtime builder must be called once")?;
    assert!(
        passed.is_none(),
        "open_session must pass None when no initialize was received"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// agent name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_open_session_passes_configured_agent_name() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_agent_name(temp.path()).await?;
    let manager = manager.with_agent_name(Some("dev".to_string()));
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd.clone(), Vec::new()).await?;
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    assert_eq!(
        captured,
        vec![Some("dev".to_string()), Some("dev".to_string())],
        "every session must carry the configured agent name"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_open_session_passes_none_agent_name_by_default() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_agent_name(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let passed = captured
        .first()
        .ok_or("the runtime builder must be called once")?;
    assert!(
        passed.is_none(),
        "without a configured agent name the params must carry None"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// get_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_get_session_unknown_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let session = manager.get_session("no-such-session");

    // -- Check
    assert!(session.is_none(), "an unknown id must not resolve");
    Ok(())
}

// ---------------------------------------------------------------------------
// list_sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_list_sessions_includes_new_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert!(
        listed.iter().any(|info| info.id == session_id),
        "list_sessions must include the new session; got: {listed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_sessions_are_independent() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let first_cwd = session_cwd(temp.path(), "first-cwd")?;
    let second_cwd = session_cwd(temp.path(), "second-cwd")?;

    // -- Exec
    let first = manager.new_session(first_cwd.clone(), Vec::new()).await?;
    let second = manager.new_session(second_cwd.clone(), Vec::new()).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert_ne!(first, second, "each session must get its own id");
    assert_eq!(listed.len(), 2, "both sessions must be listed");
    let first_session = manager
        .get_session(&first)
        .ok_or("first session must exist")?;
    let second_session = manager
        .get_session(&second)
        .ok_or("second session must exist")?;
    assert_eq!(first_session.cwd(), first_cwd.as_path());
    assert_eq!(second_session.cwd(), second_cwd.as_path());
    Ok(())
}

// ---------------------------------------------------------------------------
// close_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_close_session_frees_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.close_session(&session_id).await?;

    // -- Check
    assert!(
        manager.get_session(&session_id).is_none(),
        "a closed session must be freed"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_close_session_unknown_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.close_session("no-such-session").await;

    // -- Check
    let err = result.err().ok_or("closing an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// delete_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_delete_session_removes_from_list() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.delete_session(&session_id).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert!(
        !listed.iter().any(|info| info.id == session_id),
        "a deleted session must not be listed; got: {listed:?}"
    );
    assert!(
        manager.get_session(&session_id).is_none(),
        "a deleted session must be freed"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_delete_session_keeps_other_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let first_cwd = session_cwd(temp.path(), "first-cwd")?;
    let second_cwd = session_cwd(temp.path(), "second-cwd")?;
    let first = manager.new_session(first_cwd, Vec::new()).await?;
    let second = manager.new_session(second_cwd, Vec::new()).await?;

    // -- Exec
    manager.delete_session(&first).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert_eq!(listed.len(), 1, "only the deleted session must be gone");
    assert_eq!(listed[0].id, second);
    Ok(())
}

#[tokio::test]
async fn test_session_manager_delete_session_deletes_the_rig_log() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, rig) = manager_with_rig_store(temp.path()).await?;
    let session_id = "persisted-session";
    seed_session(&rig, session_id)?;
    assert!(
        rig.holds(session_id)?,
        "the fixture must start with a persisted rig log"
    );

    // -- Exec
    manager.delete_session(session_id).await?;

    // -- Check
    assert!(
        !rig.holds(session_id)?,
        "deleting a session must delete its rig log, not orphan it"
    );
    Ok(())
}

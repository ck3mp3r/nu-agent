//! Session lifecycle bus events: `SessionEvent::Started` on open and
//! `SessionEvent::Ended` on close/delete.
//!
//! The manager publishes on the session's own bus, so each test builds the
//! manager with a bus it created itself and subscribes before the session is
//! opened — a broadcast channel does not buffer for later subscribers.

use super::*;

/// Drain every event currently buffered on the session channel.
fn take_session_events(rx: &mut agnt_core::bus::SessionRx) -> Vec<SessionEvent> {
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
async fn test_session_manager_open_session_publishes_started() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let bus = create_bus();
    let manager = manager_with_bus(temp.path(), bus.clone()).await?;
    let mut session_rx = bus.session().subscribe();
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let events = take_session_events(&mut session_rx);
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::Started { session_id: id, hydrated } if id == &session_id && !*hydrated
        )),
        "opening a fresh session must publish Started with hydrated=false; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_open_session_publishes_started_hydrated_for_a_persisted_session()
-> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let bus = create_bus();
    let (manager, rig) = manager_with_bus_and_rig_store(temp.path(), bus.clone()).await?;
    let mut session_rx = bus.session().subscribe();
    let session_id = "persisted-session";
    seed_session(&rig, session_id)?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.resume_session(session_id, cwd).await?;

    // -- Check
    let events = take_session_events(&mut session_rx);
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::Started { session_id: id, hydrated } if id == session_id && *hydrated
        )),
        "resuming a persisted session must publish Started with hydrated=true; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_close_session_publishes_ended() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let bus = create_bus();
    let manager = manager_with_bus(temp.path(), bus.clone()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut session_rx = bus.session().subscribe();

    // -- Exec
    manager.close_session(&session_id).await?;

    // -- Check
    let events = take_session_events(&mut session_rx);
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::Ended { session_id: id } if id == &session_id
        )),
        "closing a live session must publish Ended with its id; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_delete_session_publishes_ended_for_a_live_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let bus = create_bus();
    let manager = manager_with_bus(temp.path(), bus.clone()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut session_rx = bus.session().subscribe();

    // -- Exec
    manager.delete_session(&session_id).await?;

    // -- Check
    let events = take_session_events(&mut session_rx);
    assert!(
        events.iter().any(|event| matches!(
            event,
            SessionEvent::Ended { session_id: id } if id == &session_id
        )),
        "deleting a live session must publish Ended with its id; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_delete_session_without_a_live_session_publishes_no_ended()
-> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let bus = create_bus();
    let (manager, rig) = manager_with_bus_and_rig_store(temp.path(), bus.clone()).await?;
    let mut session_rx = bus.session().subscribe();
    let session_id = "persisted-session";
    seed_session(&rig, session_id)?;

    // -- Exec
    manager.delete_session(session_id).await?;

    // -- Check
    let events = take_session_events(&mut session_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::Ended { .. })),
        "deleting a session that is not live must publish no Ended; got: {events:?}"
    );
    Ok(())
}

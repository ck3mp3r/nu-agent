use super::*;

/// `expire_status_message_if_due` clears an expired message, keeps a fresh
/// message, and keeps a warning within its TTL.
#[test]
fn expire_status_message_if_due_contract() -> Result<()> {
    // -- Setup & Fixtures: an expired message.
    let mut expired = RuntimeCoordinator::new(80, 24, None);
    expired.state.status.message.set_message("stale");

    // -- Exec & Check
    assert!(expired.expire_status_message_if_due(Instant::now() + Duration::from_secs(5)));
    assert!(expired.state.status.message.status_line().is_empty());

    // -- Setup & Fixtures: a fresh message.
    let mut fresh = RuntimeCoordinator::new(80, 24, None);
    fresh.state.status.message.set_message("fresh");

    // -- Exec & Check
    assert!(!fresh.expire_status_message_if_due(Instant::now()));
    assert_eq!(fresh.state.status.message.status_line(), "fresh");

    // -- Setup & Fixtures: a warning within its TTL.
    let mut warning = RuntimeCoordinator::new(80, 24, None);
    warning.state.status.message.set_warning("warned");

    // -- Exec & Check
    assert!(!warning.expire_status_message_if_due(Instant::now() + Duration::from_secs(5)));
    assert_eq!(warning.state.status.message.status_line(), "warned");
    Ok(())
}

/// `has_pending_status_message` is false when fresh, true after a message is
/// set, and false again once expiry clears it.
#[test]
fn has_pending_status_message_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let coord = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check: false when fresh.
    assert!(!coord.has_pending_status_message());

    // -- Setup & Fixtures
    let mut set = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check: true after a message is set.
    set.state.status.message.set_message("info");
    assert!(set.has_pending_status_message());

    // -- Setup & Fixtures
    let mut expired = RuntimeCoordinator::new(80, 24, None);
    expired.state.status.message.set_message("stale");

    // -- Exec
    let did_expire = expired.expire_status_message_if_due(Instant::now() + Duration::from_secs(5));

    // -- Check: false after expiry clears.
    assert!(did_expire);
    assert!(!expired.has_pending_status_message());
    Ok(())
}

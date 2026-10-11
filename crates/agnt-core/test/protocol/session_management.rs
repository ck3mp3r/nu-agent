use super::session_management::{SessionPersistence, SessionState};
use crate::session::SessionInfo;
use std::path::Path;

// --- SessionState tests ---

struct NoopSessionState;

impl SessionState for NoopSessionState {}

#[test]
fn session_state_defaults_are_no_ops() {
    let mut state = NoopSessionState;
    // Call all three methods — verify no panic.
    state.clear_session();
    state.new_session();
    state.seed_last_total_tokens(None);
    state.seed_last_total_tokens(Some(42));
}

// --- SessionPersistence tests ---

struct NoopSessionPersistence;

impl SessionPersistence for NoopSessionPersistence {}

#[tokio::test]
async fn session_persistence_default_load_session_returns_error() {
    let mut persistence = NoopSessionPersistence;
    let result = persistence.load_session("test").await;
    assert_eq!(result, Err("Session loading not supported".to_string()));
}

#[tokio::test]
async fn session_persistence_default_list_sessions_returns_empty() {
    let persistence = NoopSessionPersistence;
    let result = persistence.list_sessions(Path::new("/")).await;
    assert_eq!(result, Ok(Vec::<SessionInfo>::new()));
}

// --- Combined implementation test ---

struct CombinedSession;

impl SessionState for CombinedSession {}

impl SessionPersistence for CombinedSession {}

#[tokio::test]
async fn both_traits_can_be_implemented_together() {
    let mut combined = CombinedSession;

    // Sync methods
    combined.clear_session();
    combined.new_session();
    combined.seed_last_total_tokens(Some(100));

    // Async methods
    let load_result = combined.load_session("test").await;
    assert_eq!(
        load_result,
        Err("Session loading not supported".to_string())
    );

    let list_result = combined.list_sessions(Path::new("/")).await;
    assert_eq!(list_result, Ok(Vec::<SessionInfo>::new()));
}

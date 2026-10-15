use super::session_management::{SessionPersistence, SessionState};
use crate::session::SessionInfo;
use std::path::Path;

struct NoopSessionState;

impl SessionState for NoopSessionState {}

struct NoopSessionPersistence;

impl SessionPersistence for NoopSessionPersistence {}

struct CombinedSession;

impl SessionState for CombinedSession {}

impl SessionPersistence for CombinedSession {}

#[tokio::test]
async fn session_trait_defaults_are_no_ops() {
    // SessionState defaults are no-ops.
    let mut state = NoopSessionState;
    state.clear_session();
    state.new_session();
    state.seed_last_total_tokens(None);
    state.seed_last_total_tokens(Some(42));

    // SessionPersistence defaults report unsupported loading and an empty listing.
    let mut persistence = NoopSessionPersistence;
    let result = persistence.load_session("test").await;
    assert_eq!(result, Err("Session loading not supported".to_string()));
    let result = persistence.list_sessions(Path::new("/")).await;
    assert_eq!(result, Ok(Vec::<SessionInfo>::new()));

    // Both traits can be implemented together.
    let mut combined = CombinedSession;
    combined.clear_session();
    combined.new_session();
    combined.seed_last_total_tokens(Some(100));
    let load_result = combined.load_session("test").await;
    assert_eq!(
        load_result,
        Err("Session loading not supported".to_string())
    );
    let list_result = combined.list_sessions(Path::new("/")).await;
    assert_eq!(list_result, Ok(Vec::<SessionInfo>::new()));
}

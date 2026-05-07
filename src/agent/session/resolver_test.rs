use nu_protocol::LabeledError;

use crate::agent::session::resolver::{
    DefaultSessionResolver, SessionRequest, SessionResolutionInput, SessionResolver,
    resolve_session_request,
};
use crate::session::SessionStore;

#[test]
fn resolve_session_request_matrix_tui_with_explicit_session_attaches() {
    let req = resolve_session_request(true, Some("abc".to_string()), false);
    assert_eq!(req, SessionRequest::Attach("abc".to_string()));
}

#[test]
fn resolve_session_request_matrix_tui_with_explicit_session_and_new_session_still_attaches() {
    let req = resolve_session_request(true, Some("abc".to_string()), true);
    assert_eq!(req, SessionRequest::Attach("abc".to_string()));
}

#[test]
fn resolve_session_request_matrix_tui_without_session_creates() {
    let req = resolve_session_request(true, None, false);
    match req {
        SessionRequest::Create(id) => assert!(id.starts_with("session-")),
        other => panic!("expected create, got {other:?}"),
    }
}

#[test]
fn resolve_session_request_matrix_non_tui_without_flags_is_none() {
    let req = resolve_session_request(false, None, false);
    assert_eq!(req, SessionRequest::None);
}

#[test]
fn resolve_session_request_matrix_non_tui_new_session_creates() {
    let req = resolve_session_request(false, None, true);
    match req {
        SessionRequest::Create(id) => assert!(id.starts_with("session-")),
        other => panic!("expected create, got {other:?}"),
    }
}

#[test]
fn default_session_resolver_tui_explicit_loads_existing_else_creates_missing() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let resolver = DefaultSessionResolver::new(&store);

    let first = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            input_is_nothing: true,
            session_id: Some("tui-explicit".to_string()),
            new_session: false,
        })
        .expect("first resolve");

    assert_eq!(first.final_session_id.as_deref(), Some("tui-explicit"));
    let mut first_session = first.session.expect("session created");
    assert_eq!(first_session.id(), "tui-explicit");
    assert!(!first.tui_should_hydrate_transcript);
    first_session
        .add_message(
            &store,
            crate::session::Message::new("user".to_string(), "persist me".to_string()),
        )
        .expect("persist history");

    let second = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            input_is_nothing: true,
            session_id: Some("tui-explicit".to_string()),
            new_session: false,
        })
        .expect("second resolve");

    assert_eq!(second.final_session_id.as_deref(), Some("tui-explicit"));
    let second_session = second.session.expect("session loaded");
    assert_eq!(second_session.id(), "tui-explicit");
    assert!(second.tui_should_hydrate_transcript);
}

#[test]
fn default_session_resolver_tui_without_session_auto_creates_and_skips_hydration() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let resolver = DefaultSessionResolver::new(&store);

    let resolved = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            input_is_nothing: true,
            session_id: None,
            new_session: false,
        })
        .expect("resolve");

    assert!(resolved.final_session_id.is_some());
    assert!(resolved.session.is_some());
    assert!(!resolved.tui_should_hydrate_transcript);
}

#[test]
fn default_session_resolver_non_tui_no_flags_returns_none() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let resolver = DefaultSessionResolver::new(&store);

    let resolved = resolver
        .resolve(SessionResolutionInput {
            use_tui: false,
            input_is_nothing: false,
            session_id: None,
            new_session: false,
        })
        .expect("resolve");

    assert!(resolved.final_session_id.is_none());
    assert!(resolved.session.is_none());
}

#[test]
fn default_session_resolver_errors_on_non_not_found_tui_load_failure() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("not-a-dir");
    std::fs::write(&file_path, "x").expect("write file");
    let store = SessionStore::new_with_cache_dir(file_path);
    let resolver = DefaultSessionResolver::new(&store);

    let result: Result<_, LabeledError> = resolver.resolve(SessionResolutionInput {
        use_tui: true,
        input_is_nothing: true,
        session_id: Some("will-fail".to_string()),
        new_session: false,
    });

    assert!(result.is_err());
}

use super::*;

/// `resolve` prefixes both user-provided and auto-generated session ids with
/// the cwd-derived directory prefix.
#[tokio::test]
async fn resolve_session_request_prefixes_ids() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use tempfile::TempDir;

    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let resolver = DefaultSessionResolver::new(store);
    let cwd = std::path::PathBuf::from("/home/user/project");
    let prefix = crate::session::prefix::dir_prefix(&cwd);

    // -- Exec & Check: a user-provided id is prefixed
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: false,
            session_id: Some("foo".to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();
    let id = result.final_session_id.unwrap();
    assert_eq!(&id[..17], format!("{prefix}-"));
    assert!(
        id.ends_with("foo"),
        "expected id to end with 'foo', got: {id}"
    );

    // -- Exec & Check: an auto-generated id is prefixed
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: None,
            cwd: cwd.clone(),
        })
        .await
        .unwrap();
    let id = result.final_session_id.unwrap();
    assert!(
        id.starts_with(&format!("{prefix}-")),
        "expected id to start with '{prefix}-', got: {id}"
    );
}

/// Attaching an existing session loads its messages and leaves
/// `last_total_tokens` unset.
#[tokio::test]
async fn attach_existing_session_returns_initial_messages_and_no_tokens() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use crate::types::Message;
    use tempfile::TempDir;

    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let cwd = std::path::PathBuf::from("/home/user/project");
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let raw_id = "existing-session";
    let prefixed_id = format!("{prefix}-{raw_id}");
    let messages = vec![Message::user("hello"), Message::assistant("world")];
    store.create(&prefixed_id, &messages).await.unwrap();

    // -- Exec
    let resolver = DefaultSessionResolver::new(Arc::clone(&store));
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some(raw_id.to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    // -- Check
    assert!(
        result.should_hydrate_transcript,
        "should_hydrate_transcript must be true when session has messages"
    );
    assert!(
        !result.initial_messages.is_empty(),
        "initial_messages must be non-empty when session has messages"
    );
    assert!(
        result.last_total_tokens.is_none(),
        "last_total_tokens must be None for existing sessions, got: {:?}",
        result.last_total_tokens
    );
}

#[tokio::test]
async fn attach_falls_back_to_legacy_prefixed_session() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use crate::types::Message;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let cwd = std::path::PathBuf::from("/home/user/project");

    // Create a session with the legacy 7-char prefix (as if created before the
    // prefix length increase).
    let legacy_prefix = crate::session::prefix::dir_prefix_legacy(&cwd);
    let raw_id = "old-session";
    let legacy_id = format!("{legacy_prefix}-{raw_id}");
    store
        .create(&legacy_id, &[Message::user("hello")])
        .await
        .unwrap();

    let resolver = DefaultSessionResolver::new(Arc::clone(&store));
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some(raw_id.to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    // The resolved final_session_id must be the legacy-prefixed ID
    assert_eq!(
        result.final_session_id.as_deref(),
        Some(legacy_id.as_str()),
        "expected legacy-prefixed session to be resolved via fallback"
    );
    assert!(
        result.should_hydrate_transcript,
        "should_hydrate_transcript must be true when legacy session has messages"
    );
}

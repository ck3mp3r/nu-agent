use super::*;

// ================================================================
// List
// ================================================================

#[tokio::test]
async fn list_returns_newest_first() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Create 3 sessions in order. `created_at` is an RFC 3339 timestamp with
    // sub-second precision, so sequential awaited creates are strictly ordered
    // and no settle between them is needed.
    store
        .create("session-oldest", &[Message::user("oldest")])
        .await
        .expect("create oldest");

    store
        .create("session-middle", &[Message::user("middle")])
        .await
        .expect("create middle");

    store
        .create("session-newest", &[Message::user("newest")])
        .await
        .expect("create newest");

    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 3, "should list all 3 sessions");

    // Newest first
    assert_eq!(
        sessions[0].id, "session-newest",
        "newest session should be first"
    );
    assert_eq!(
        sessions[1].id, "session-middle",
        "middle session should be second"
    );
    assert_eq!(
        sessions[2].id, "session-oldest",
        "oldest session should be last"
    );
}

#[tokio::test]
async fn list_filters_zero_entry_sessions() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Create a session with entries
    store
        .create("session-with-data", &[Message::user("data")])
        .await
        .expect("create with data");

    // Create a session row with no entries by appending empty
    store
        .append("session-empty", &[])
        .await
        .expect("append empty");

    // List should only include the session with entries
    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 1, "should filter out zero-entry sessions");
    assert_eq!(sessions[0].id, "session-with-data");
}

// ================================================================
// Delete
// ================================================================

#[tokio::test]
async fn delete_removes_session_and_entries() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    store
        .create("delete-me", &[Message::user("bye")])
        .await
        .expect("create");

    // Verify it exists
    assert!(
        store.load("delete-me").await.expect("load").is_some(),
        "session should exist before delete"
    );

    // Delete
    store.delete("delete-me").await.expect("delete");

    // Verify it's gone
    assert!(
        store.load("delete-me").await.expect("load").is_none(),
        "session should not exist after delete"
    );

    // List should be empty
    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 0, "list should be empty after delete");
}

#[tokio::test]
async fn delete_nonexistent_is_noop() {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    // Deleting a non-existent session should not error
    let result = store.delete("i-never-existed").await;
    assert!(
        result.is_ok(),
        "deleting non-existent session should be Ok(())"
    );
}

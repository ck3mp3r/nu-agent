use super::*;

// ================================================================
// Pool connection persistence
// ================================================================

#[tokio::test]
async fn memory_store_persists_sessions_table_across_queries() {
    // Regression test: the :memory: SQLite database is per-connection.
    // If the pool drops its single connection, the next query opens a new
    // connection with a fresh empty database and the sessions table is lost.
    // min_connections(1) keeps the connection alive for the pool's lifetime.
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    store
        .create("persist-test", &[Message::user("hello")])
        .await
        .expect("create");

    let sessions = store.list().await.expect("list");
    assert_eq!(sessions.len(), 1, "session should persist across queries");
    assert_eq!(sessions[0].id, "persist-test");
}

// ================================================================
// Tool-verdict flag carrier round-trip
// ================================================================

/// A ToolResult carrying the `is_error` verdict must survive the SQLite
/// Message serde round trip unchanged — the store needs no schema change to
/// carry the flag.
#[tokio::test]
async fn sqlite_round_trips_tool_verdict_flag() -> Result<()> {
    // -- Setup & Fixtures
    let store = SqliteSessionStore::new(":memory:")
        .await
        .map_err(|e| format!("create store: {e:?}"))?;
    let stamped = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("tc1"),
            name: ToolName::new("test_tool").expect("non-empty tool name"),
            content: vec![ToolResultContent::Text(Text::new("the tool failed"))],
            is_error: true,
        })],
    };

    // -- Exec
    store
        .create("verdict-session", &[stamped])
        .await
        .map_err(|e| format!("create: {e:?}"))?;
    let (_, entries) = store
        .load("verdict-session")
        .await
        .map_err(|e| format!("load: {e:?}"))?
        .ok_or("session should load")?;

    // -- Check
    let StoreEntry::Message(Message::User { content }) = &entries[0] else {
        panic!("expected User message entry, got: {:?}", entries[0]);
    };
    let Some(UserContent::ToolResult(tr)) = content.first() else {
        panic!("expected ToolResult content, got: {content:?}");
    };
    assert!(
        tr.is_error,
        "persisted verdict must survive the SQLite serde round trip"
    );
    Ok(())
}

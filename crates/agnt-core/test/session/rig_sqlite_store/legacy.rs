use super::*;

/// A database of the session schema nu-agent kept before rig-memory is
/// refused, and left as it is.
#[tokio::test]
async fn legacy_database_is_refused() -> Result<()> {
    // -- Setup & Fixtures: the old `sessions`/`entries` schema
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    let path_str = path.to_str().ok_or("the path is UTF-8")?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, title TEXT NOT NULL DEFAULT '')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TABLE entries (session_id TEXT NOT NULL, seq INTEGER NOT NULL, entry_type TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY (session_id, seq))",
    )
    .execute(&pool)
    .await?;
    pool.close().await;

    // -- Exec
    let refused = RigSqliteStore::open(path_str, "session-legacy");

    // -- Check
    let error = refused.err().ok_or("a legacy database is refused")?;
    assert!(
        matches!(&error, RigStoreError::Legacy),
        "the error names the legacy layout: {error}"
    );
    assert!(
        error.to_string().contains("start a new session"),
        "the error says what to do: {error}"
    );
    Ok(())
}

//! Tests of the storage engine, its transport faces, and the migration runner.

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Modules

use sqlx::SqlitePool;
use tempfile::TempDir;

use crate::{Storage, StorageError, Work};

// endregion: --- Modules

// region:    --- Tests

#[test]
fn test_storage_open_creates_file_and_migrates() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("nested").join("sessions.db");

    // -- Exec
    let storage = Storage::open(&path)?;

    // -- Check
    assert!(path.exists(), "open SHALL create the database file");
    let version = storage.call_blocking(Version)??;
    assert_eq!(version, 2, "open SHALL migrate a new database to M1");
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(
        tables,
        [
            "entry_turns",
            "files",
            "logs",
            "sessions",
            "tool_calls",
            "turns"
        ]
    );
    Ok(())
}

#[test]
fn test_storage_open_migrates_nothing_at_target_version() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    drop(Storage::open(&path)?);

    // -- Exec
    // The M1 statements are plain `CREATE TABLE`: a second migration run fails.
    let storage = Storage::open(&path)?;

    // -- Check
    let version = storage.call_blocking(Version)??;
    assert_eq!(version, 2);
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(
        tables,
        [
            "entry_turns",
            "files",
            "logs",
            "sessions",
            "tool_calls",
            "turns"
        ]
    );
    Ok(())
}

#[test]
fn test_storage_open_refuses_newer_schema() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    let storage = Storage::open(&path)?;
    storage.call_blocking(SetVersion(7))??;
    drop(storage);

    // -- Exec
    let refusal = Storage::open(&path)
        .err()
        .ok_or("opening a newer database SHALL fail")?;

    // -- Check
    let message = refusal.to_string();
    let StorageError::SchemaTooNew { stored, target } = refusal else {
        return Err(format!("expected a version refusal, got {message}").into());
    };
    assert_eq!((stored, target), (7, 2));
    assert!(
        message.contains("version 7") && message.contains("the 2"),
        "the refusal SHALL name both versions: {message}"
    );
    Ok(())
}

#[test]
fn test_storage_open_memory_path_stays_in_memory() -> Result<()> {
    // -- Setup & Fixtures
    let first = Storage::open(":memory:")?;
    let second = Storage::open(":memory:")?;
    first.call_blocking(InsertSession {
        id: "a91f3c2d".to_owned(),
    })??;

    // -- Exec
    let ids = second.call_blocking(SessionIds)??;

    // -- Check
    assert_eq!(
        ids,
        Vec::<String>::new(),
        "each memory open SHALL be its own database"
    );
    Ok(())
}

#[test]
fn test_storage_in_memory_carries_schema() -> Result<()> {
    // -- Setup & Fixtures
    let storage = Storage::in_memory()?;

    // -- Exec & Check
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(
        tables,
        [
            "entry_turns",
            "files",
            "logs",
            "sessions",
            "tool_calls",
            "turns"
        ]
    );

    storage.call_blocking(InsertSession {
        id: "a91f3c2d".to_owned(),
    })??;
    let ids = storage.call_blocking(SessionIds)??;
    assert_eq!(ids, ["a91f3c2d"], "both work units SHALL see one database");
    Ok(())
}

#[test]
fn test_storage_open_two_handles_one_file() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    let first = Storage::open(&path)?;
    let second = Storage::open(&path)?;

    // -- Exec
    first.call_blocking(InsertSession {
        id: "a91f3c2d".to_owned(),
    })??;
    second.call_blocking(InsertSession {
        id: "b04e7710".to_owned(),
    })??;

    // -- Check
    let ids = second.call_blocking(SessionIds)??;
    assert_eq!(ids, ["a91f3c2d", "b04e7710"]);
    Ok(())
}

#[tokio::test]
async fn test_storage_call_awaits_reply() -> Result<()> {
    // -- Setup & Fixtures
    let storage = Storage::in_memory()?;

    // -- Exec
    let version = storage.call(Version).await??;

    // -- Check
    assert_eq!(version, 2);
    Ok(())
}

// endregion: --- Tests

// region:    --- Support

/// Answers the schema version the database stores.
struct Version;

impl Work for Version {
    type Output = sqlx::Result<i64>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await
    }
}

/// Answers the names of the tables the database holds, in order.
struct Tables;

impl Work for Tables {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}

/// Writes the schema version the database stores.
struct SetVersion(i64);

impl Work for SetVersion {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let version = self.0;
        sqlx::query(&format!("PRAGMA user_version = {version}"))
            .execute(&pool)
            .await?;
        Ok(())
    }
}

/// The legacy pre-engine store is refused before any DDL, with a clean named
/// error that leaves the file exactly as it was.
#[tokio::test]
async fn test_storage_open_refuses_the_legacy_sqlx_migration_store() -> Result<()> {
    // -- Setup & Fixtures: the deleted `sqlx::migrate!` era's file, version 0.
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    let pool = legacy_pool(&path).await?;
    sqlx::query("CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY)")
        .execute(&pool)
        .await?;
    sqlx::query("CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, made INTEGER)")
        .execute(&pool)
        .await?;
    sqlx::query("CREATE TABLE entries (id INTEGER PRIMARY KEY, session_id TEXT)")
        .execute(&pool)
        .await?;
    pool.close().await;

    // -- Exec
    let refusal = Storage::open(&path)
        .err()
        .ok_or("opening the legacy store SHALL fail")?;

    // -- Check: the named refusal names the era, both versions and the path.
    let message = refusal.to_string();
    let StorageError::LegacyStore {
        stored,
        target,
        path: refused_path,
    } = refusal
    else {
        return Err(format!("expected a legacy-store refusal, got {message}").into());
    };
    assert_eq!((stored, target), (0, 2));
    assert_eq!(refused_path, path);
    assert!(
        message.contains("legacy sqlx-migration store")
            && message.contains("stored schema version 0")
            && message.contains("migrates to 2"),
        "the refusal SHALL name the era and both versions: {message}"
    );
    assert!(
        message.contains(&path.display().to_string()),
        "the refusal SHALL name the resolved path: {message}"
    );
    assert!(
        message.contains("left untouched"),
        "the refusal SHALL state the file was left untouched: {message}"
    );
    assert!(
        !message.contains("error returned from database"),
        "the refusal SHALL not leak the sqlx wording: {message}"
    );

    // -- Check: zero CREATE statements ran — the legacy tables are all there is.
    let pool = legacy_pool(&path).await?;
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await?;
    pool.close().await;
    assert_eq!(
        tables.into_iter().map(|(name,)| name).collect::<Vec<_>>(),
        ["_sqlx_migrations", "entries", "sessions"],
        "the refusal SHALL run no DDL and leave the legacy file as it was"
    );
    Ok(())
}

/// A pool over the database at `path`, created when it is missing.
async fn legacy_pool(path: &std::path::Path) -> sqlx::Result<SqlitePool> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    sqlx::SqlitePool::connect_with(options).await
}

/// Writes one session row.
struct InsertSession {
    id: String,
}

impl Work for InsertSession {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        sqlx::query("INSERT INTO sessions (id, made_millis) VALUES (?1, ?2)")
            .bind(&self.id)
            .bind(1_790_974_800_213_i64)
            .execute(&pool)
            .await?;
        Ok(())
    }
}

/// Answers the ids the `sessions` table holds, in order.
struct SessionIds;

impl Work for SessionIds {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions ORDER BY id")
            .fetch_all(&pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
}

// endregion: --- Support

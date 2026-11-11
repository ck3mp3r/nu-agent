//! Tests of the storage engine, its transport faces, and its migrations.

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Modules

use std::path::Path;

use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use tempfile::TempDir;

use crate::{Storage, StorageError, Work};

// endregion: --- Modules

// region:    --- Fixtures

/// The tables a migrated database holds: the engine's, and the framework's
/// migration bookkeeping.
const ENGINE_TABLES: [&str; 8] = [
    "_sqlx_migrations",
    "entry_turns",
    "files",
    "logs",
    "sessions",
    "tool_calls",
    "transcript",
    "turns",
];

// endregion: --- Fixtures

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
    let versions = storage.call_blocking(AppliedVersions)??;
    assert_eq!(
        versions,
        [1, 2, 3, 4],
        "open SHALL apply every migration of a new database"
    );
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(tables, ENGINE_TABLES);
    Ok(())
}

#[test]
fn test_storage_open_migrates_nothing_at_target_version() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    drop(Storage::open(&path)?);

    // -- Exec
    // Every migration's statements run once: a second run would fail, and the
    // framework's applied rows are what keep them from running.
    let storage = Storage::open(&path)?;

    // -- Check
    let versions = storage.call_blocking(AppliedVersions)??;
    assert_eq!(versions, [1, 2, 3, 4]);
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(tables, ENGINE_TABLES);
    Ok(())
}

#[tokio::test]
async fn test_storage_open_refuses_a_version_the_build_does_not_know() -> Result<()> {
    // -- Setup & Fixtures: a database a newer build has migrated.
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    drop(Storage::open(&path)?);
    let pool = open_pool(&path).await?;
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (7, 'a migration of a newer build', TRUE, X'00', 0)",
    )
    .execute(&pool)
    .await?;
    pool.close().await;

    // -- Exec
    let refusal = Storage::open(&path)
        .err()
        .ok_or("opening a newer database SHALL fail")?;

    // -- Check
    let message = refusal.to_string();
    let StorageError::Migrate(MigrateError::VersionMissing(version)) = refusal else {
        return Err(
            format!("expected the framework's missing-version refusal, got {message}").into(),
        );
    };
    assert_eq!(version, 7, "the refusal SHALL name the unknown version");
    assert!(
        message.contains("migration 7 was previously applied"),
        "the refusal SHALL carry the framework's text: {message}"
    );
    Ok(())
}

#[tokio::test]
async fn test_storage_open_refuses_a_foreign_database() -> Result<()> {
    // -- Setup & Fixtures: a foreign database, holding no migration bookkeeping.
    let dir = TempDir::new()?;
    let path = dir.path().join("sessions.db");
    let pool = open_pool(&path).await?;
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
        .ok_or("opening a foreign database SHALL fail")?;

    // -- Check: the first migration collides with the foreign `sessions` table.
    let message = refusal.to_string();
    let StorageError::Migrate(MigrateError::ExecuteMigration(_, version)) = refusal else {
        return Err(format!("expected the framework's migration failure, got {message}").into());
    };
    assert_eq!(version, 1, "the failure SHALL name the colliding migration");
    assert!(
        message.contains("table sessions already exists"),
        "the refusal SHALL carry the collision: {message}"
    );

    // -- Check: the failed migration rolled back whole, leaving the foreign
    // tables beside the framework's empty bookkeeping table.
    let pool = open_pool(&path).await?;
    let tables = tables(&pool).await?;
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    pool.close().await;
    assert_eq!(tables, ["_sqlx_migrations", "entries", "sessions"]);
    assert_eq!(applied, 0, "the failed migration SHALL record nothing");
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
    assert_eq!(tables, ENGINE_TABLES);

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
    let versions = storage.call(AppliedVersions).await??;

    // -- Check
    assert_eq!(versions, [1, 2, 3, 4]);
    Ok(())
}

// endregion: --- Tests

// region:    --- Support

/// Answers the migration versions the framework records as applied.
struct AppliedVersions;

impl Work for AppliedVersions {
    type Output = sqlx::Result<Vec<i64>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
    }
}

/// Answers the names of the tables the database holds, in order.
struct Tables;

impl Work for Tables {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        tables(&pool).await
    }
}

/// A pool over the database at `path`, created when it is missing.
async fn open_pool(path: &Path) -> sqlx::Result<SqlitePool> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    sqlx::SqlitePool::connect_with(options).await
}

/// The names of the tables `pool` holds, in order.
async fn tables(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
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

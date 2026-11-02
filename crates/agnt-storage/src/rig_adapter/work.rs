//! The work units rig's contracts cross the engine with, and their SQL.
//!
//! rig's blob tables keep their shape (D1: rig owns the bytes' meaning). A
//! persisting write records its session in the same transaction, so the
//! `sessions` row and the bytes it describes can never disagree.

// region:    --- Modules

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::sqlite::SqlitePool;
use sqlx::{Sqlite, Transaction};

use crate::engine::Work;

// endregion: --- Modules

// region:    --- Types

/// Replaces the file `name` with `bytes`.
pub(super) struct WriteFile {
    pub session: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Appends `bytes` to the file `name`, creating it when there is none.
pub(super) struct AppendFile {
    pub session: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The file `name`; `None` when there is none.
pub(super) struct ReadFile {
    pub session: String,
    pub name: String,
}

/// Removes the file `name`.
pub(super) struct RemoveFile {
    pub session: String,
    pub name: String,
}

/// The names of the session's files, in order.
pub(super) struct FileNames {
    pub session: String,
}

/// The names of the session's logs, in order.
pub(super) struct LogNames {
    pub session: String,
}

/// The log `name`; `None` when there is none.
pub(super) struct ReadLog {
    pub session: String,
    pub name: String,
}

/// Appends `bytes` to the log `name` if it is `len` bytes long now, answering
/// whether it landed.
pub(super) struct AppendLog {
    pub session: String,
    pub name: String,
    pub len: i64,
    pub bytes: Vec<u8>,
}

/// Replaces the log `name` with `bytes` if it is `len` bytes long now,
/// answering whether it landed.
pub(super) struct ReplaceLog {
    pub session: String,
    pub name: String,
    pub len: i64,
    pub bytes: Vec<u8>,
}

/// Removes everything the session holds, and the session itself.
pub(super) struct Destroy {
    pub session: String,
}

/// The ids of the sessions the database holds, in order.
pub(super) struct SessionIds;

// endregion: --- Types

// region:    --- Files

impl Work for WriteFile {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n             ON CONFLICT(session_id, name) DO UPDATE SET bytes = excluded.bytes",
        )
        .bind(&self.session)
        .bind(&self.name)
        .bind(&self.bytes)
        .execute(&mut *tx)
        .await?;
        record_session(&mut tx, &self.session).await?;
        tx.commit().await
    }
}

impl Work for AppendFile {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO files (session_id, name, bytes) VALUES (?1, ?2, ?3)\n             ON CONFLICT(session_id, name)\n             DO UPDATE SET bytes = CAST(bytes || excluded.bytes AS BLOB)",
        )
        .bind(&self.session)
        .bind(&self.name)
        .bind(&self.bytes)
        .execute(&mut *tx)
        .await?;
        record_session(&mut tx, &self.session).await?;
        tx.commit().await
    }
}

impl Work for ReadFile {
    type Output = sqlx::Result<Option<Vec<u8>>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT bytes FROM files WHERE session_id = ?1 AND name = ?2")
                .bind(&self.session)
                .bind(&self.name)
                .fetch_optional(&pool)
                .await?;
        Ok(row.map(|(bytes,)| bytes))
    }
}

impl Work for RemoveFile {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        sqlx::query("DELETE FROM files WHERE session_id = ?1 AND name = ?2")
            .bind(&self.session)
            .bind(&self.name)
            .execute(&pool)
            .await?;
        Ok(())
    }
}

impl Work for FileNames {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM files WHERE session_id = ?1 ORDER BY name")
                .bind(&self.session)
                .fetch_all(&pool)
                .await?;
        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}

// endregion: --- Files

// region:    --- Logs

impl Work for LogNames {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM logs WHERE session_id = ?1 ORDER BY name")
                .bind(&self.session)
                .fetch_all(&pool)
                .await?;
        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}

impl Work for ReadLog {
    type Output = sqlx::Result<Option<Vec<u8>>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT bytes FROM logs WHERE session_id = ?1 AND name = ?2")
                .bind(&self.session)
                .bind(&self.name)
                .fetch_optional(&pool)
                .await?;
        Ok(row.map(|(bytes,)| bytes))
    }
}

impl Work for AppendLog {
    type Output = sqlx::Result<bool>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        let started = if self.len == 0 {
            sqlx::query(
                "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                 ON CONFLICT(session_id, name) DO NOTHING",
            )
            .bind(&self.session)
            .bind(&self.name)
            .bind(&self.bytes)
            .execute(&mut *tx)
            .await?
            .rows_affected()
        } else {
            0
        };
        if started > 0 {
            record_session(&mut tx, &self.session).await?;
            tx.commit().await?;
            return Ok(true);
        }
        let appended = sqlx::query(
            "UPDATE logs SET bytes = CAST(bytes || ?3 AS BLOB)\n             WHERE session_id = ?1 AND name = ?2\n             AND length(CAST(bytes AS BLOB)) = ?4",
        )
        .bind(&self.session)
        .bind(&self.name)
        .bind(&self.bytes)
        .bind(self.len)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if appended == 0 {
            return Ok(false);
        }
        record_session(&mut tx, &self.session).await?;
        tx.commit().await?;
        Ok(true)
    }
}

impl Work for ReplaceLog {
    type Output = sqlx::Result<bool>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        let started = if self.len == 0 {
            sqlx::query(
                "INSERT INTO logs (session_id, name, bytes) VALUES (?1, ?2, ?3)\n                 ON CONFLICT(session_id, name) DO NOTHING",
            )
            .bind(&self.session)
            .bind(&self.name)
            .bind(&self.bytes)
            .execute(&mut *tx)
            .await?
            .rows_affected()
        } else {
            0
        };
        if started > 0 {
            record_session(&mut tx, &self.session).await?;
            tx.commit().await?;
            return Ok(true);
        }
        let replaced = sqlx::query(
            "UPDATE logs SET bytes = ?3\n             WHERE session_id = ?1 AND name = ?2\n             AND length(CAST(bytes AS BLOB)) = ?4",
        )
        .bind(&self.session)
        .bind(&self.name)
        .bind(&self.bytes)
        .bind(self.len)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if replaced == 0 {
            return Ok(false);
        }
        record_session(&mut tx, &self.session).await?;
        tx.commit().await?;
        Ok(true)
    }
}

impl Work for Destroy {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM logs WHERE session_id = ?1")
            .bind(&self.session)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM files WHERE session_id = ?1")
            .bind(&self.session)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(&self.session)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
}

// endregion: --- Logs

// region:    --- Sessions

impl Work for SessionIds {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions ORDER BY id")
            .fetch_all(&pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
}

// endregion: --- Sessions

// region:    --- Support

/// Records `session`, keeping the instant it first persisted anything.
async fn record_session(tx: &mut Transaction<'_, Sqlite>, session: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, made_millis) VALUES (?1, ?2)\n         ON CONFLICT(id) DO NOTHING",
    )
    .bind(session)
    .bind(now_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The instant now, in milliseconds since the Unix epoch.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

// endregion: --- Support

//! The `user_version` migration runner.

// region:    --- Modules

use sqlx::SqlitePool;

use crate::engine::Location;
use crate::error::{Result, StorageError};
use crate::schema::{M1, M2, M3, M4};

// endregion: --- Modules

// region:    --- Constants

/// The migrations this build knows, oldest first: index `n` migrates version
/// `n` to version `n + 1`.
pub(crate) const MIGRATIONS: &[&[&str]] = &[M1, M2, M3, M4];

/// The `user_version` this build migrates a database to.
pub(crate) const TARGET_VERSION: u32 = MIGRATIONS.len() as u32;

// endregion: --- Constants

// region:    --- Support

/// Migrates the database from its stored version to [`TARGET_VERSION`] in one
/// transaction, refusing a database that is newer than this build or that
/// belongs to the legacy pre-engine store.
pub(crate) async fn migrate(pool: &SqlitePool, location: &Location) -> Result<()> {
    let stored = user_version(pool).await?;
    if stored > TARGET_VERSION {
        return Err(StorageError::SchemaTooNew {
            stored,
            target: TARGET_VERSION,
        });
    }
    if stored == TARGET_VERSION {
        return Ok(());
    }
    // A database that stores no schema version yet holding tables is not this
    // engine's: it is the legacy sqlx-migration store, whose `sessions` table
    // collides with the first migration. Refuse before any DDL, so the file is
    // left exactly as it was.
    if stored == 0
        && let Some(_) = foreign_table(pool).await?
    {
        return Err(StorageError::LegacyStore {
            stored,
            target: TARGET_VERSION,
            path: location.path(),
        });
    }
    let mut tx = pool.begin().await?;
    for (index, statements) in MIGRATIONS.iter().enumerate().skip(stored as usize) {
        for statement in statements.iter() {
            sqlx::query(statement).execute(&mut *tx).await?;
        }
        let version = index as u32 + 1;
        sqlx::query(&format!("PRAGMA user_version = {version}"))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The first table the database holds that this engine did not create.
///
/// Every engine table is created by a migration, so at version 0 a table can
/// only be someone else's — or a leftover of the pre-engine store.
async fn foreign_table(pool: &SqlitePool) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?)
}

/// The schema version the database at `pool` stores.
async fn user_version(pool: &SqlitePool) -> Result<u32> {
    let stored: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;
    u32::try_from(stored).map_err(|_| {
        StorageError::Custom(format!(
            "the database stores an unreadable schema version: {stored}"
        ))
    })
}

// endregion: --- Support

//! The M2 fact tables and the services that write and read them.

// region:    --- Modules

#[path = "entries.rs"]
mod entries;

#[path = "schema.rs"]
mod schema;

#[path = "settle.rs"]
mod settle;

#[path = "tool_calls.rs"]
mod tool_calls;

#[path = "turns.rs"]
mod turns;

// endregion: --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{
    CallFact, Entries, EntryTurn, MessageKey, Settle, SettleRecord, Storage, ToolCallStatus,
    ToolCalls, TurnFacts, Turns, Work,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The session every fixture writes under.
const SESSION: &str = "session-m2";

/// One in-memory database and the three services over it.
struct Fixture {
    storage: Arc<Storage>,
}

impl Fixture {
    fn new() -> Result<Self> {
        Ok(Self {
            storage: Arc::new(Storage::in_memory()?),
        })
    }

    fn turns(&self) -> Turns {
        Turns::new(Arc::clone(&self.storage))
    }

    fn entries(&self) -> Entries {
        Entries::new(Arc::clone(&self.storage))
    }

    fn tool_calls(&self) -> ToolCalls {
        ToolCalls::new(Arc::clone(&self.storage))
    }

    fn settle(&self) -> Settle {
        Settle::new(Arc::clone(&self.storage))
    }

    /// The facts of a settled turn: every column known.
    fn turn_facts(&self, turn_seq: i64) -> TurnFacts {
        TurnFacts {
            session_id: SESSION.to_owned(),
            turn_seq,
            submitted_millis: 1_790_974_800_213 + turn_seq,
            settled_millis: 1_790_974_804_987 + turn_seq,
            model: "anthropic/claude-sonnet-4-5".to_owned(),
            agent: "architect".to_owned(),
        }
    }

    /// A turn recorded, so entry and call rows have their parent.
    async fn settled_turn(&self, turn_seq: i64) -> Result<TurnFacts> {
        let facts = self.turn_facts(turn_seq);
        self.turns().record(facts.clone()).await?;
        Ok(facts)
    }
}

/// The schema version the database stores.
async fn version(storage: &Storage) -> Result<i64> {
    Ok(storage.call(Version).await??)
}

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

/// The names of the tables the database holds, in order.
async fn tables(storage: &Storage) -> Result<Vec<String>> {
    Ok(storage.call(Tables).await??)
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

/// The columns of `table`: each name and whether it is `NOT NULL`.
async fn columns(storage: &Storage, table: &str) -> Result<Vec<(String, bool)>> {
    Ok(storage.call(Columns(table.to_owned())).await??)
}

/// Answers the columns of one table with their `NOT NULL` flags.
struct Columns(String);

impl Work for Columns {
    type Output = sqlx::Result<Vec<(String, bool)>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT name, \"notnull\" FROM pragma_table_info(?1)")
                .bind(self.0)
                .fetch_all(&pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(name, not_null)| (name, not_null != 0))
            .collect())
    }
}

/// The `CREATE TABLE` statement of `table`.
async fn table_sql(storage: &Storage, table: &str) -> Result<String> {
    Ok(storage.call(TableSql(table.to_owned())).await??)
}

/// Answers the `CREATE TABLE` statement of one table.
struct TableSql(String);

impl Work for TableSql {
    type Output = sqlx::Result<String>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .bind(self.0)
            .fetch_one(&pool)
            .await
    }
}

/// A settled call's facts, every column known.
fn call_fact(turn_seq: i64, call: &str, status: ToolCallStatus) -> CallFact {
    CallFact {
        session_id: SESSION.to_owned(),
        call: call.to_owned(),
        turn_seq,
        status,
        settled_millis: 1_790_974_803_999,
    }
}

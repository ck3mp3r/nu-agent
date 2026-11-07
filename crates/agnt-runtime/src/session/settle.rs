//! The turn's settle: the turn row, the entry keys and the call rows.
//!
//! One settled turn is one transaction. The facts the turn recorded and rig's
//! key for each entry it persisted travel together, so a reload reads a turn
//! and the entries it holds, or neither. The ordinal is the count of the
//! session's settled turns: this module assigns it, the services receive it.

// region:    --- Modules

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agnt_storage::{
    CallFact, Entries, EntryTurn, MessageKey, Settle, SettleRecord, Storage, ToolCallStatus,
    ToolCalls, TurnFacts, Turns,
};

use super::MemoryError;
use super::blocking::on_pool;
use super::factory::RigStoreConfig;
use super::tool_status::{ToolStatus, ToolStatusMap};
use crate::hook::TurnToolRecord;

// endregion:    --- Modules

// region:    --- Types

/// What one finished turn records, as the settle path hands it over.
#[derive(Debug)]
pub struct SettleInput<'a> {
    /// The session the turn belongs to.
    pub session_id: &'a str,
    /// The model identity the turn ran under, `vendor/model`.
    pub model: &'a str,
    /// The persona that answered the turn, empty when none was loaded.
    pub agent: &'a str,
    /// When the turn was submitted, in milliseconds since the Unix epoch.
    pub submitted_millis: i64,
    /// rig's key for each entry the turn persisted, `None` when the pre-turn
    /// log could not be read.
    ///
    /// The keys are a difference against what the log held when the turn
    /// started. When that read fails there is nothing to subtract: an empty
    /// "before" would name the session's whole history as this turn's entries.
    pub keys: Option<Vec<MessageKey>>,
    /// Every call the turn dispatched, in dispatch order.
    pub records: &'a [TurnToolRecord],
}

/// The facts one session holds, as a reload reads them.
#[derive(Debug, Default)]
pub struct SessionFacts {
    /// The entry attributions, by rig key.
    pub entries: Vec<EntryTurn>,
    /// The statuses the session's calls settled with.
    pub calls: Vec<CallFact>,
    /// The session's settled turns, in turn order: the recorded submit instant
    /// and the identities each turn ran under.
    pub turns: Vec<TurnFacts>,
}

impl SessionFacts {
    /// The status each of the session's calls settled with, by call id.
    pub fn status_map(&self) -> ToolStatusMap {
        self.calls
            .iter()
            .map(|call| (call.call.clone(), tool_status(call.status)))
            .collect()
    }

    /// The turn each entry was persisted under, by rig key.
    pub fn entry_turns(&self) -> HashMap<MessageKey, i64> {
        self.entries
            .iter()
            .map(|entry| (entry.key, entry.turn_seq))
            .collect()
    }
}

/// The fact tables of one database.
pub struct FactStore {
    storage: Arc<Storage>,
}

// endregion:    --- Types

// region:    --- Constructors

impl FactStore {
    /// The fact tables of `config`'s database.
    ///
    /// The services write through the same SQLite file the session's rig log
    /// lives in. A `Memory` config opens an in-memory database of its own: it
    /// keeps nothing across the store's opens, so neither do its facts.
    ///
    /// # Errors
    ///
    /// [`super::RigStoreError`] when the database cannot be opened.
    pub async fn open(config: &RigStoreConfig) -> Result<Self, super::RigStoreError> {
        let config = config.clone();
        let storage = on_pool(move || config.facts_blocking())
            .await
            .map_err(super::RigStoreError::from)??;
        Ok(Self { storage })
    }
}

// endregion:    --- Constructors

// region:    --- Operations

impl FactStore {
    /// Records `input` as one settle: the turn row, its entry keys, its calls.
    ///
    /// A cancelled turn records nothing: its prompt and its settled calls are
    /// persisted by recovery instead, which settles the turn it closes.
    ///
    /// A turn whose pre-turn log could not be read records nothing either: its
    /// entries are unknown, and attributing the whole log to it would repoint
    /// the session's history. The conversation survives; only this turn's facts
    /// do not land.
    ///
    /// # Errors
    ///
    /// [`MemoryError`] when the facts cannot be read or the unit is refused.
    pub async fn settle(&self, input: SettleInput<'_>) -> Result<(), MemoryError> {
        let Some(keys) = input.keys else {
            log::warn!(
                "Skipping the settle of session '{}': its pre-turn log could not be read, so \
                 the turn's entries are unknown",
                input.session_id
            );
            return Ok(());
        };
        let turns = self.turns(input.session_id).await?;
        let turn_seq = turns
            .iter()
            .map(|turn| turn.turn_seq)
            .max()
            .map_or(1, |max| max + 1);
        let settled_millis = now_millis();
        let calls = input
            .records
            .iter()
            .map(|record| CallFact {
                session_id: input.session_id.to_owned(),
                call: record.call_id.to_string(),
                turn_seq,
                status: call_status(record.status.unwrap_or(ToolStatus::Cancelled)),
                settled_millis,
            })
            .collect();
        let record = SettleRecord {
            turn: TurnFacts {
                session_id: input.session_id.to_owned(),
                turn_seq,
                submitted_millis: input.submitted_millis,
                settled_millis,
                model: input.model.to_owned(),
                agent: input.agent.to_owned(),
            },
            keys,
            calls,
        };
        Settle::new(Arc::clone(&self.storage))
            .record(record)
            .await
            .map_err(MemoryError::backend)
    }

    /// Everything `session_id` recorded, as a reload reads it.
    ///
    /// # Errors
    ///
    /// [`MemoryError`] when a fact table cannot be read.
    pub async fn read(&self, session_id: &str) -> Result<SessionFacts, MemoryError> {
        Ok(SessionFacts {
            entries: Entries::new(Arc::clone(&self.storage))
                .read(session_id)
                .await
                .map_err(MemoryError::backend)?,
            calls: ToolCalls::new(Arc::clone(&self.storage))
                .read(session_id)
                .await
                .map_err(MemoryError::backend)?,
            turns: self.turns(session_id).await?,
        })
    }

    /// The agent of the session's most recent recorded turn.
    ///
    /// The most recent turn is the one with the highest `turn_seq`. A session
    /// with no recorded turn answers `None`; a turn that ran persona-less
    /// records the empty string, which is likewise no agent.
    pub async fn last_turn_agent(&self, session_id: &str) -> Result<Option<String>, MemoryError> {
        let last = self
            .turns(session_id)
            .await?
            .into_iter()
            .max_by_key(|turn| turn.turn_seq);
        Ok(last
            .map(|turn| turn.agent)
            .filter(|agent| !agent.trim().is_empty()))
    }

    /// The session's settled turns, in turn order.
    async fn turns(&self, session_id: &str) -> Result<Vec<TurnFacts>, MemoryError> {
        Turns::new(Arc::clone(&self.storage))
            .read(session_id)
            .await
            .map_err(MemoryError::backend)
    }
}

// endregion:    --- Operations

// region:    --- Support

/// The table's status for the turn's structural one.
fn call_status(status: ToolStatus) -> ToolCallStatus {
    match status {
        ToolStatus::Success => ToolCallStatus::Success,
        ToolStatus::Failed => ToolCallStatus::Failed,
        ToolStatus::Cancelled => ToolCallStatus::Cancelled,
    }
}

/// The turn's structural status for the one the table stored.
fn tool_status(status: ToolCallStatus) -> ToolStatus {
    match status {
        ToolCallStatus::Success => ToolStatus::Success,
        ToolCallStatus::Failed => ToolStatus::Failed,
        ToolCallStatus::Cancelled => ToolStatus::Cancelled,
    }
}

/// The current instant, in milliseconds since the Unix epoch.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// The keys `saved` holds beyond `before`: the entries the turn persisted.
pub fn new_keys(saved: &[MessageKey], before: Option<&[MessageKey]>) -> Option<Vec<MessageKey>> {
    let before = before?;
    Some(
        saved
            .iter()
            .copied()
            .filter(|key| !before.contains(key))
            .collect(),
    )
}

/// rig's keys of `entries`, as the database stores them.
pub fn entry_keys<'a>(
    entries: impl IntoIterator<Item = rig::conversation::EntryRef<'a>>,
) -> Vec<MessageKey> {
    entries
        .into_iter()
        .map(|entry| MessageKey {
            made: entry.made as i64,
            id: entry.id.get() as i64,
        })
        .collect()
}

// endregion:    --- Support

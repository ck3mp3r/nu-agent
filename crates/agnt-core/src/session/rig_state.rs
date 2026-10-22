//! Host-written session state records and the listing cache.
//!
//! rig 0.45's save path writes only the header and change records. The state
//! records (`rig::model`, `rig::title`) and the `meta.json` listing are the
//! host's to write. Both go through the sanctioned session-level writer:
//! `Session::read` → edit the main agent's saved values → `Session::save`,
//! which dedups unchanged values and refuses stale writes.

use std::collections::BTreeSet;

use rig_memory::{MODEL, Repair, Session, SessionMeta, Store, TITLE};

use super::MemoryError;
use crate::types::{Message, UserContent};

/// What the session commands show of a session: its listing, its main agent's
/// model and title, its conversation, and what reading it repaired.
#[derive(Debug, Default)]
pub struct SessionView {
    /// The session's `meta.json` listing; the default when it has none.
    pub meta: SessionMeta,
    /// The main agent's model, `vendor/model`.
    pub model: Option<String>,
    /// The main agent's title.
    pub title: Option<String>,
    /// When the main agent's log was started, in milliseconds since the Unix
    /// epoch.
    pub started_ms: Option<u64>,
    /// The main agent's conversation, as rig reads it.
    pub messages: Vec<Message>,
    /// What reading the session repaired or worked around.
    pub repairs: Vec<Repair>,
}

/// Read the session in `store` whole: its listing, its main agent's model,
/// title and conversation, and what reading it repaired.
///
/// # Errors
///
/// When the store cannot be read.
pub fn read_view(store: &dyn Store) -> Result<SessionView, MemoryError> {
    let session = Session::read(store).map_err(MemoryError::backend)?;
    let (model, title, started_ms, messages) = match session.main_agent() {
        Some(agent) => (
            agent.model(),
            agent.title(),
            Some(agent.started_ms),
            agent
                .conversation
                .messages()
                .iter()
                .map(|message| (**message).clone())
                .collect(),
        ),
        None => (None, None, None, Vec::new()),
    };
    Ok(SessionView {
        meta: SessionMeta::read(store).unwrap_or_default(),
        model,
        title,
        started_ms,
        messages,
        repairs: session.repairs,
    })
}

/// Persist `model` and `title` as the main agent's state records, then refresh
/// the session's `meta.json` listing.
///
/// A repeated write with unchanged values adds no record: `Session::save`
/// dedups them against the log.
///
/// # Errors
///
/// When the store cannot be read or written.
pub fn write_state_records(
    store: &dyn Store,
    model: &str,
    title: Option<&str>,
) -> Result<(), MemoryError> {
    let mut session = Session::read(store).map_err(MemoryError::backend)?;
    let Some(main) = session.main_agent().map(|agent| agent.id.clone()) else {
        log::debug!("No main agent to write state records for");
        return Ok(());
    };
    let Some(agent) = session.agents.iter_mut().find(|agent| agent.id == main) else {
        return Ok(());
    };
    agent.saved.insert(
        MODEL.to_string(),
        serde_json::Value::String(model.to_string()),
    );
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        agent.saved.insert(
            TITLE.to_string(),
            serde_json::Value::String(title.to_string()),
        );
    }
    session.save(store).map_err(MemoryError::backend)?;
    relist(store, &session, title)
}

/// Refresh the session's `meta.json` listing from `session`.
///
/// `relist` keeps the title and labels, so a non-empty `title` is set first.
fn relist(store: &dyn Store, session: &Session, title: Option<&str>) -> Result<(), MemoryError> {
    let mut meta = SessionMeta::read(store).unwrap_or_default();
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        meta.title = title.to_string();
    }
    meta.relist(session, &BTreeSet::new());
    meta.write(store).map_err(MemoryError::backend)
}

/// The title of a conversation: the start of its first user message.
pub fn title_of_messages(messages: &[Message]) -> Option<String> {
    messages.iter().find_map(|message| match message {
        Message::User { content } => content.iter().find_map(|item| match item {
            UserContent::Text(text) => Some(rig_memory::title_of(&text.text)),
            _ => None,
        }),
        _ => None,
    })
}

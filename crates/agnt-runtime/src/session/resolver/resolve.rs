//! Which session a run attaches to or creates, and the store it opens.

// region:    --- Modules

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nu_protocol::LabeledError;

use super::hydration::hydrate_transcript_messages;
use super::read_entries;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::{RigStoreConfig, Session};

// endregion: --- Modules

// region:    --- Types

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    None,
    Attach(String),
    Create(String),
}

#[derive(Debug, Clone)]
pub struct SessionResolutionInput {
    pub use_tui: bool,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SessionResolution {
    pub final_session_id: Option<String>,
    pub session: Option<Session>,
    pub should_hydrate_transcript: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

pub trait SessionResolver {
    fn resolve(
        &self,
        input: SessionResolutionInput,
    ) -> impl std::future::Future<Output = Result<SessionResolution, LabeledError>> + Send;
}

pub struct DefaultSessionResolver {
    rig: RigStoreConfig,
}

// endregion: --- Types

// region:    --- Constructors

impl DefaultSessionResolver {
    pub fn new(rig: RigStoreConfig) -> Self {
        Self { rig }
    }
}

// endregion: --- Constructors

// region:    --- Session

impl SessionResolver for DefaultSessionResolver {
    async fn resolve(
        &self,
        input: SessionResolutionInput,
    ) -> Result<SessionResolution, LabeledError> {
        let prefix = crate::session::prefix::dir_prefix(&input.cwd);
        let request = resolve_session_request(input.use_tui, input.session_id);
        let request = match request {
            SessionRequest::Attach(id) => SessionRequest::Attach(format!("{prefix}-{id}")),
            SessionRequest::Create(id) => SessionRequest::Create(format!("{prefix}-{id}")),
            SessionRequest::None => SessionRequest::None,
        };
        match request {
            SessionRequest::None => Ok(SessionResolution {
                final_session_id: None,
                session: None,
                should_hydrate_transcript: false,
                initial_messages: Vec::new(),
                last_total_tokens: None,
            }),
            SessionRequest::Attach(id) => {
                let store = self.open(&id).await?;
                let existed_before_attach = holds(&store, &id).await?;
                // Token count is not estimated here — rig's CompactingMemory
                // tracks the context and last_total_tokens stays None on attach.
                let initial_messages = if existed_before_attach {
                    hydrate_transcript(&self.rig, &store, &id, &input.cwd).await?
                } else {
                    Vec::new()
                };

                Ok(SessionResolution {
                    final_session_id: Some(id.clone()),
                    session: Some(Session::new(id)),
                    should_hydrate_transcript: !initial_messages.is_empty(),
                    initial_messages,
                    last_total_tokens: None,
                })
            }
            SessionRequest::Create(id) => Ok(SessionResolution {
                final_session_id: Some(id.clone()),
                session: Some(Session::new(id)),
                should_hydrate_transcript: false,
                initial_messages: Vec::new(),
                last_total_tokens: None,
            }),
        }
    }
}

impl DefaultSessionResolver {
    /// The rig-memory store of `session_id`.
    async fn open(&self, session_id: &str) -> Result<Arc<dyn rig_memory::Store>, LabeledError> {
        self.rig
            .open(session_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to open session '{session_id}': {e}")))
    }
}

// endregion: --- Session

// region:    --- Functions

/// The transcript of `session_id`, read from the store's own record: the fold
/// writes the session's blocks as rows, and the rows answer the hydration.
pub(crate) async fn hydrate_transcript(
    rig: &RigStoreConfig,
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
    cwd: &Path,
) -> Result<Vec<UiMessageSnapshot>, LabeledError> {
    let (entries, statuses, turns) = read_entries(rig, store, session_id).await?;
    Ok(hydrate_transcript_messages(
        &entries, cwd, &statuses, &turns,
    ))
}

pub fn generate_session_id() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!(
        "{}-{}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_micros()
    )
}

pub fn resolve_session_request(use_tui: bool, session_id: Option<String>) -> SessionRequest {
    match (use_tui, session_id) {
        // TUI explicit session policy is centralized here:
        // always attempt to attach first (resolver handles load-then-create fallback).
        (true, Some(id)) => SessionRequest::Attach(id),
        (_, Some(id)) => SessionRequest::Create(id),
        (true, None) => SessionRequest::Create(generate_session_id()),
        (false, None) => SessionRequest::None,
    }
}

// endregion: --- Functions

// region:    --- Support

/// Whether `store` holds a log for `session_id`.
async fn holds(store: &Arc<dyn rig_memory::Store>, session_id: &str) -> Result<bool, LabeledError> {
    let store = Arc::clone(store);
    let session = session_id.to_owned();
    let logs = crate::session::blocking::on_pool(move || store.logs())
        .await
        .map_err(|e| LabeledError::new(format!("Failed to list session logs '{session}': {e}")))?
        .map_err(|e| LabeledError::new(format!("Failed to list session logs '{session}': {e}")))?;
    Ok(logs.iter().any(|log| log == session_id))
}

// endregion: --- Support

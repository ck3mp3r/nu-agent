//! The picker catalogs: the personas a surface offers and the sessions it can
//! switch to.
//!
//! Both reads are stateless over handles a runtime already holds, so a picker
//! refresh is answered from here instead of through the worker: the catalog
//! fills while a turn runs.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::AgentsConfig;
use crate::protocol::persona::{FsPersonaResolver, PersonaLister, PersonaSummary};
use crate::session::{CachedMemory, RigStoreConfig, SessionInfo};

// region:    --- Types

/// Reads the agent and session catalogs from the handles a runtime holds.
///
/// The reader owns every input its two reads need — the scan root, the persona
/// configuration, the session store handle, and the session the runtime is
/// attached to — so answering a picker needs no runtime and no worker turn.
#[derive(Clone)]
pub struct PickerCatalog {
    cwd: PathBuf,
    agents_dir: PathBuf,
    agents_config: AgentsConfig,
    memory: Arc<CachedMemory>,
    /// The session the runtime is attached to, which the listing omits.
    ///
    /// The listing omits it because the user is already there. The snapshot is
    /// built on the session the runtime starts in, and every confirmed switch
    /// moves the omission with it — the loop applies
    /// [`Self::with_current_session`] on the switch spine row, so a surface
    /// switch and a peer attach both keep it current.
    current_session: Option<String>,
}

/// Runtime capability: the reader that answers a picker refresh.
///
/// Defaults to a catalog with no scan root and no session store: its agent read
/// lists the enabled built-in personas alone, and its session read lists
/// nothing.
pub trait PickerCatalogSource {
    fn picker_catalog(&self) -> PickerCatalog {
        PickerCatalog::default()
    }
}

// endregion: --- Types

// region:    --- Froms

impl Default for PickerCatalog {
    fn default() -> Self {
        Self {
            cwd: PathBuf::new(),
            agents_dir: PathBuf::new(),
            agents_config: AgentsConfig::default(),
            memory: Arc::new(CachedMemory::new(RigStoreConfig::in_memory())),
            current_session: None,
        }
    }
}

// endregion: --- Froms

// region:    --- Constructors

impl PickerCatalog {
    /// A catalog rooted at `cwd`, reading the persona chain from `agents_dir`
    /// and the sessions from `memory`.
    pub(crate) fn new(
        cwd: PathBuf,
        agents_dir: PathBuf,
        agents_config: AgentsConfig,
        memory: Arc<CachedMemory>,
        current_session: Option<String>,
    ) -> Self {
        Self {
            cwd,
            agents_dir,
            agents_config,
            memory,
            current_session,
        }
    }

    /// The same catalog with its working directory replaced.
    ///
    /// The session listing is CWD-scoped: a caller that names a working
    /// directory lists the sessions that belong to it.
    pub(crate) fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// The same catalog with the session its listing omits replaced.
    ///
    /// The snapshot is built when the loop starts, but the runtime moves to
    /// another session mid-loop: without this the picker hides the session the
    /// user just left and offers the one they are already in.
    pub(crate) fn with_current_session(mut self, current: Option<String>) -> Self {
        self.current_session = current;
        self
    }
}

// endregion: --- Constructors

// region:    --- Read

impl PickerCatalog {
    /// The agent catalog: a fresh persona scan of the cwd chain and the persona
    /// directory, so a persona added or deleted since startup is listed.
    pub(crate) fn list_agents(&self) -> Vec<PersonaSummary> {
        let resolver = FsPersonaResolver::new(
            self.cwd.clone(),
            self.agents_dir.clone(),
            self.agents_config.clone(),
        );
        resolver.list_available()
    }

    /// The session catalog for this catalog's working directory.
    ///
    /// A session with no messages is not usable: a model switch writes its
    /// state records before the first turn, so it would otherwise list as an
    /// empty, untitled session. The current session is where the user already
    /// is, so it is not offered as a target.
    pub(crate) async fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let ids = self
            .memory
            .session_ids()
            .await
            .map_err(|e| format!("Failed to list sessions: {e}"))?;
        let current = self.current_session.as_deref();
        // One facts read serves the whole listing: the agent column is the last
        // recorded turn's. A session whose facts cannot be read shows a blank
        // cell rather than a guessed one.
        let facts = match self.memory.facts().await {
            Ok(facts) => Some(facts),
            Err(error) => {
                log::warn!("failed to open the session fact tables for the listing: {error}");
                None
            }
        };
        let mut sessions = Vec::new();
        for id in ids {
            if current == Some(id.as_str()) {
                continue;
            }
            let Ok(store) = self.memory.rig_store(&id).await else {
                // One unreadable session must not hide the others.
                log::warn!("failed to open the session store of '{id}'");
                continue;
            };
            let read = crate::session::blocking::on_pool(move || {
                Ok::<rig_memory::SessionMeta, core::convert::Infallible>(
                    rig_memory::SessionMeta::read(&*store).unwrap_or_default(),
                )
            })
            .await;
            let meta = match read {
                Ok(Ok(meta)) => meta,
                Ok(Err(cannot_happen)) => match cannot_happen {},
                Err(error) => {
                    log::warn!("failed to read the session listing of '{id}': {error}");
                    continue;
                }
            };
            if meta.messages == 0 {
                continue;
            }
            // The recorded change instant, or nothing: an unknown stamp shows a
            // blank cell, never the wall clock.
            let last_active = match meta.updated {
                0 => None,
                ms => chrono::DateTime::from_timestamp_millis(ms as i64),
            };
            let agent = match facts.as_ref() {
                Some(facts) => facts.last_turn_agent(&id).await.unwrap_or_else(|error| {
                    log::warn!("failed to read the last turn of '{id}': {error}");
                    None
                }),
                None => None,
            };
            sessions.push(SessionInfo {
                id,
                message_count: meta.messages,
                last_active,
                title: (!meta.title.is_empty()).then_some(meta.title),
                agent,
            });
        }
        Ok(crate::session::prefix::filter_sessions_by_cwd(
            sessions, &self.cwd,
        ))
    }
}

// endregion: --- Read

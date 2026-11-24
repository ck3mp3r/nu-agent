//! The session lifecycle: open, load, resume, list, close, and delete.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agent_client_protocol::schema::v1::LoadSessionResponse;
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::session::SessionInfo;
use agnt_runtime::session::resolver::generate_session_id;
use agnt_runtime::tools::mcp::config::McpServerConfig;
use agnt_runtime::types::Message;
use chrono::Utc;
use rig_memory::Store as _;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{AcpError, Result};
use crate::runtime::AcpRuntimeParams;
use crate::translator::SessionUpdateSink;

use super::support::replay_history;
use super::types::{AcpSession, AcpSessionManager, RuntimeBuilder};

impl<R: RuntimeBuilder> AcpSessionManager<R> {
    /// Open a new session for `cwd` and return its id.
    ///
    /// The id is generated here and handed to the runtime as its persisted
    /// session id, so the conversation is stored under the id the client sees.
    pub(crate) async fn new_session(
        &self,
        cwd: impl Into<PathBuf>,
        mcp_servers: Vec<McpServerConfig>,
    ) -> Result<String> {
        let session_id = generate_session_id();
        self.open_session(&session_id, cwd.into(), mcp_servers)
            .await?;
        Ok(session_id)
    }

    /// Replay the stored conversation of `session_id` to `sink` and make the
    /// session live again.
    ///
    /// This is the ACP `session/load` handler: the client gets the whole
    /// conversation as `session/update` notifications, and the session is
    /// registered so the next prompt runs against the restored history.
    /// Returns [`AcpError::SessionNotFound`] when the store has no such session.
    pub(crate) async fn load_session<S: SessionUpdateSink>(
        &self,
        session_id: &str,
        cwd: impl Into<PathBuf>,
        sink: &S,
    ) -> Result<LoadSessionResponse> {
        let entries = self.load_entries(session_id).await?;
        self.open_session(session_id, cwd.into(), Vec::new())
            .await?;
        replay_history(sink, session_id, &entries)?;
        Ok(LoadSessionResponse::new())
    }

    /// Make the stored session `session_id` live again without replaying it.
    ///
    /// This is the ACP `session/resume` handler. Returns the session id.
    /// Returns [`AcpError::SessionNotFound`] when the store has no such session.
    pub async fn resume_session(
        &self,
        session_id: &str,
        cwd: impl Into<PathBuf>,
    ) -> Result<String> {
        self.load_entries(session_id).await?;
        self.open_session(session_id, cwd.into(), Vec::new())
            .await?;
        Ok(session_id.to_string())
    }

    /// Drop the live session with `id`, freeing its runtime and closing its bus.
    ///
    /// The session stays in the store: closing is not deleting. Returns
    /// [`AcpError::SessionNotFound`] when no live session has that id.
    ///
    /// Publishes [`LifecycleEvent::SessionEnded`] on the session's bus before the bus
    /// drops, so a session-lifecycle hook sees the close.
    pub async fn close_session(&self, id: &str) -> Result<()> {
        let session = self
            .sessions()
            .remove(id)
            .ok_or_else(|| AcpError::SessionNotFound(id.to_string()))?;
        let usage = session.usage();
        let _ = session
            .channels()
            .lifecycle
            .publish(LifecycleEvent::SessionEnded {
                session_id: id.to_string(),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost: usage.cost,
                cost_complete: usage.cost_complete,
            });
        Ok(())
    }

    /// Drop the live session with `id` and delete it from the store.
    ///
    /// The session's rig-memory store is deleted with it, so no log is left
    /// orphaned. Idempotent: deleting an id that is neither live nor persisted
    /// succeeds.
    ///
    /// Publishes [`LifecycleEvent::SessionEnded`] on the session's bus when a live
    /// session was removed; a session that was never live has no bus to
    /// publish on.
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let live = self.sessions().remove(id);
        if let Some(session) = &live {
            let usage = session.usage();
            let _ = session
                .channels()
                .lifecycle
                .publish(LifecycleEvent::SessionEnded {
                    session_id: id.to_string(),
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    cost: usage.cost,
                    cost_complete: usage.cost_complete,
                });
        }
        let rig = self
            .rig
            .open(id)
            .await
            .map_err(|e| AcpError::Runtime(format!("failed to open session '{id}': {e}")))?;
        agnt_runtime::session::blocking::on_pool(move || rig.destroy())
            .await
            .map_err(|e| {
                AcpError::Runtime(format!("failed to delete the session log of '{id}': {e}"))
            })?
            .map_err(|e| {
                AcpError::Runtime(format!("failed to delete the session log of '{id}': {e}"))
            })?;
        Ok(())
    }

    /// The live sessions, newest first.
    ///
    /// Only sessions this manager holds are reported. `message_count` is 0 and
    /// `title` is `None` because the registry does not read the store — listing
    /// persisted sessions is the store's job.
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .sessions()
            .values()
            .map(|session| SessionInfo {
                id: session.id.clone(),
                message_count: 0,
                last_active: Some(session.created_at),
                title: None,
                agent: None,
            })
            .collect();
        sessions.sort_by_key(|info| std::cmp::Reverse(info.last_active));
        sessions
    }

    /// Build the runtime for `session_id` and register it as live.
    ///
    /// Replaces any live session with the same id.
    async fn open_session(
        &self,
        session_id: &str,
        cwd: PathBuf,
        mcp_servers: Vec<McpServerConfig>,
    ) -> Result<()> {
        let client_fs = self.client_fs().clone();
        // One agent snapshot per session: the runtime writes it on construction
        // and on every persona or model switch; the session's hook runner reads
        // it when it builds an envelope.
        let hook_context = agnt_runtime::hooks::HookAgentContext::default();
        let runtime = self
            .runtime_builder
            .build(AcpRuntimeParams {
                cwd: cwd.clone(),
                mcp_servers,
                paths: self.paths.clone(),
                grammar_stores: Arc::clone(&self.grammar_stores),
                runtime_handle: self.runtime_handle.clone(),
                rig: self.rig.clone(),
                session_id: Some(session_id.to_string()),
                client_fs,
                agent_name: self.agent_name.clone(),
                hook_context: hook_context.clone(),
            })
            .await?;
        let bus = runtime.bus_handle();
        let usage = runtime.session_usage_handle();
        self.sessions().insert(
            session_id.to_string(),
            Arc::new(AcpSession {
                id: session_id.to_string(),
                cwd: cwd.clone(),
                created_at: Utc::now(),
                cancel_requested: Arc::new(AtomicBool::new(false)),
                bus: bus.clone(),
                runtime: Arc::new(AsyncMutex::new(runtime)),
                usage,
            }),
        );
        // Each session owns its bus, so each session starts its own hook
        // runner. Zero hooks spawns nothing.
        if !self.hooks.is_empty() {
            let _handles = agnt_runtime::hooks::spawn_hook_runner(
                agnt_runtime::hooks::HookSources {
                    stream: bus.stream(),
                    lifecycle: bus.lifecycle(),
                },
                &self.hooks,
                &cwd,
                &hook_context,
                agnt_runtime::hooks::NuProcess,
            );
        }
        // `hydrated` reports whether the store already holds this session's
        // log, so a session-lifecycle hook can tell a fresh session from a
        // resumed one. A store that cannot be read is treated as not hydrated.
        let hydrated = self.rig.holds(session_id).await.unwrap_or(false);
        let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
            session_id: session_id.to_string(),
            hydrated,
        });
        Ok(())
    }

    /// The stored entries of `session_id`, or [`AcpError::SessionNotFound`].
    ///
    /// The conversation lives in the rig-memory store the agent writes; the
    /// session store is not consulted. `rig_memory::load` returns an empty
    /// conversation for a log that does not exist, so absence is detected from
    /// the store's log names first.
    async fn load_entries(&self, session_id: &str) -> Result<Vec<Message>> {
        let store = self.rig.open(session_id).await.map_err(|e| {
            AcpError::Runtime(format!("failed to open session '{session_id}': {e}"))
        })?;
        let session = session_id.to_owned();
        let (logs, loaded) = agnt_runtime::session::blocking::on_pool(move || {
            let logs = store.logs();
            let loaded = rig_memory::load(&*store, &session);
            (logs, loaded)
        })
        .await
        .map_err(|e| AcpError::Runtime(format!("failed to read session '{session_id}': {e}")))?;
        let logs = logs.map_err(|e| {
            AcpError::Runtime(format!("failed to list session logs '{session_id}': {e}"))
        })?;
        if !logs.iter().any(|log| log == session_id) {
            return Err(AcpError::SessionNotFound(session_id.to_string()));
        }
        let conversation = loaded.map_err(|e| {
            AcpError::Runtime(format!("failed to load session '{session_id}': {e}"))
        })?;
        Ok(conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect())
    }
}

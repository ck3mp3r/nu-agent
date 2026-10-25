//! ACP session manager: the registry of live sessions of one connection.
//!
//! ACP allows several concurrent sessions per connection, so the manager owns a
//! map of session id → [`AcpSession`]. Each session owns its own
//! `AgentConversationRuntime` (tool server, bus, model) and shares the
//! process-level session store with its siblings.

// region:    --- Modules

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, FileSystemCapabilities, LoadSessionResponse, SessionConfigKind,
    SessionConfigOption, SessionConfigSelectOptions, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_core::bus::{Bus, ChannelError, SessionEvent, TurnEvent};
use agnt_core::config::ModelsCache;
use agnt_core::conversation::managers::ProviderManager;
use agnt_core::conversation::runtime::AgentConversationRuntime;
use agnt_core::protocol::contracts::CoreRuntime;
use agnt_core::protocol::model_switching::ModelSwitching;
use agnt_core::session::resolver::generate_session_id;
use agnt_core::session::{RigStoreConfig, SessionInfo};
use agnt_core::tools::mcp::config::McpServerConfig;
use agnt_core::types::{AssistantContent, Message, UserContent};
use chrono::{DateTime, Utc};
use nu_protocol::{LabeledError, Span, Value};
use rig_memory::Store as _;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{AcpError, Result};
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};
use crate::runtime::{
    AcpRuntimeParams, build_acp_runtime, build_agent_config_options, build_model_config_options,
};
use crate::translator::{EventTranslator, SessionUpdateSink, run_turn_with_notifications};

// endregion: --- Modules

// region:    --- Types

/// How a prompt turn ended, as the ACP layer sees it.
///
/// This is the ACP boundary's own classification: `CoreRuntime::execute_turn`
/// erases its structured `TurnError` into a `LabeledError`, so the ACP layer
/// re-derives the stop category it must report to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The turn finished normally.
    Completed,
    /// The client cancelled the turn via `session/cancel`.
    Cancelled,
    /// The model hit its output-token ceiling.
    MaxTokens,
    /// The turn hit the configured tool-turn limit.
    MaxTurns,
}

/// Map a finished turn to the ACP stop reason reported to the client.
pub fn map_turn_outcome_to_stop_reason(outcome: TurnOutcome) -> StopReason {
    match outcome {
        TurnOutcome::Completed => StopReason::EndTurn,
        TurnOutcome::Cancelled => StopReason::Cancelled,
        TurnOutcome::MaxTokens => StopReason::MaxTokens,
        TurnOutcome::MaxTurns => StopReason::MaxTurnRequests,
    }
}

/// One live ACP session.
///
/// The session is shared behind an `Arc`, so `cancel()` can reach the bus and
/// the cancel flag without taking the runtime lock that an in-flight turn
/// holds. Only the runtime is behind a mutex.
pub struct AcpSession {
    id: String,
    cwd: PathBuf,
    created_at: DateTime<Utc>,
    cancel_requested: Arc<AtomicBool>,
    bus: Bus,
    runtime: Arc<AsyncMutex<AgentConversationRuntime>>,
}

impl AcpSession {
    /// The session id handed to the ACP client.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The working directory the session was created with.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The session's event bus, reachable without locking the runtime.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// The conversation runtime that drives this session.
    pub fn runtime(&self) -> &Arc<AsyncMutex<AgentConversationRuntime>> {
        &self.runtime
    }
}

/// Builds the conversation runtime of one ACP session.
///
/// The ACP boundary owns this seam so a session manager can be driven with a
/// substitute runtime builder without changing the production path, which
/// builds through [`build_acp_runtime`].
pub trait RuntimeBuilder: Send + Sync {
    /// Build the runtime for `params`.
    fn build(
        &self,
        params: AcpRuntimeParams,
    ) -> impl Future<Output = Result<AgentConversationRuntime>> + Send;
}

/// Production [`RuntimeBuilder`]: builds the runtime from config.toml.
pub struct DefaultRuntimeBuilder;

impl RuntimeBuilder for DefaultRuntimeBuilder {
    async fn build(&self, params: AcpRuntimeParams) -> Result<AgentConversationRuntime> {
        build_acp_runtime(params).await
    }
}

/// The ACP client's filesystem capabilities and the connection that reaches it.
///
/// Captured from the `initialize` request. A session runtime uses it to route
/// file reads and writes through the client instead of the local filesystem.
#[derive(Clone)]
pub struct ClientFsState {
    /// The connection back to the client, used to send `fs/*` requests.
    pub connection: ConnectionTo<Client>,
    /// The filesystem capabilities the client advertised at `initialize`.
    pub capabilities: FileSystemCapabilities,
}

/// Registry of the live sessions of one ACP connection.
///
/// Every session shares `rig`, so a session opened here can later be resumed
/// from the same store by another connection. The map itself is behind a plain
/// mutex that is only ever held for a lookup or an insert — never across an
/// await — so a long-running turn cannot block `cancel()`.
pub struct AcpSessionManager<R: RuntimeBuilder> {
    sessions: StdMutex<HashMap<String, Arc<AcpSession>>>,
    /// How the rig-memory store of a session is made. The agent writes its
    /// conversation there, so `session/load` and `session/resume` read it.
    rig: RigStoreConfig,
    config_path: PathBuf,
    runtime_handle: tokio::runtime::Handle,
    runtime_builder: R,
    models_cache: Option<Arc<ModelsCache>>,
    client_fs: Arc<StdMutex<Option<ClientFsState>>>,
    agent_name: Option<String>,
    hooks: Vec<agnt_core::config::ResolvedHook>,
}

// endregion: --- Types

// region:    --- AcpSessionManager Impl

impl AcpSessionManager<DefaultRuntimeBuilder> {
    /// Create a manager whose sessions all share `rig`.
    ///
    /// `rig` is how each session's rig-memory store is made — the store the
    /// agent writes its conversation to. `config_path` is the config.toml every
    /// session runtime is built from; `runtime_handle` is the process tokio
    /// runtime the sessions spawn on. Session runtimes are built by
    /// [`DefaultRuntimeBuilder`].
    pub fn new(
        rig: RigStoreConfig,
        config_path: impl Into<PathBuf>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        Self::with_runtime_builder(rig, config_path, runtime_handle, DefaultRuntimeBuilder)
    }
}

impl<R: RuntimeBuilder> AcpSessionManager<R> {
    /// Create a manager that builds every session runtime with `runtime_builder`.
    ///
    /// Use this to substitute the runtime builder — for example a mock builder
    /// that never touches the network.
    pub fn with_runtime_builder(
        rig: RigStoreConfig,
        config_path: impl Into<PathBuf>,
        runtime_handle: tokio::runtime::Handle,
        runtime_builder: R,
    ) -> Self {
        Self {
            sessions: StdMutex::new(HashMap::new()),
            rig,
            config_path: config_path.into(),
            runtime_handle,
            runtime_builder,
            models_cache: None,
            client_fs: Arc::new(StdMutex::new(None)),
            agent_name: None,
            hooks: Vec::new(),
        }
    }

    /// Attach the user hooks every session starts a runner for.
    ///
    /// The hooks are resolved once at serve time; each session spawns its own
    /// runner on its own bus, so a hook observes that session's events only.
    pub fn with_hooks(mut self, hooks: Vec<agnt_core::config::ResolvedHook>) -> Self {
        self.hooks = hooks;
        self
    }

    /// Attach the models cache used to enumerate selectable models.
    ///
    /// Without a cache the model selector advertises only the configured model.
    pub fn with_models_cache(mut self, cache: Option<Arc<ModelsCache>>) -> Self {
        self.models_cache = cache;
        self
    }

    /// Attach the shared client filesystem state captured at `initialize`.
    ///
    /// The state is shared with the `initialize` handler, which fills it once
    /// the client advertises its filesystem capabilities. Without it, session
    /// runtimes fall back to the local filesystem.
    pub fn with_client_fs(mut self, state: Arc<StdMutex<Option<ClientFsState>>>) -> Self {
        self.client_fs = state;
        self
    }

    /// Attach the agent whose permissions overlay applies to every session.
    ///
    /// `None` falls back to the configured default agent. The name is carried
    /// into every [`AcpRuntimeParams`] this manager builds.
    pub fn with_agent_name(mut self, agent_name: Option<String>) -> Self {
        self.agent_name = agent_name;
        self
    }

    /// Open a new session for `cwd` and return its id.
    ///
    /// The id is generated here and handed to the runtime as its persisted
    /// session id, so the conversation is stored under the id the client sees.
    pub async fn new_session(
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
    pub async fn load_session<S: SessionUpdateSink>(
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

    /// The model config options advertised for `session_id`.
    ///
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn model_config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        Ok(model_options(&runtime, self.models_cache.as_deref()))
    }

    /// The agent config options advertised for `session_id`.
    ///
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn agent_config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        Ok(agent_options(&runtime))
    }

    /// The agent and model config options advertised for `session_id`.
    ///
    /// The agent option comes first. Every response that changes one selector
    /// carries both, so the client never drops the other.
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        let mut options = agent_options(&runtime);
        options.extend(model_options(&runtime, self.models_cache.as_deref()));
        Ok(options)
    }

    /// Switch `session_id` to `model_value` and return the updated config options.
    ///
    /// `model_value` must be one of the values advertised by
    /// [`Self::model_config_options`]; anything else is rejected with
    /// [`AcpError::Config`]. The model is rebuilt from the session's cached
    /// provider client, so no new transport is created. Returns
    /// [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn set_model(
        &self,
        session_id: &str,
        model_value: &str,
    ) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let mut runtime = session.runtime.lock().await;
        let options = model_options(&runtime, self.models_cache.as_deref());
        if !select_values(&options)
            .iter()
            .any(|value| value == model_value)
        {
            return Err(AcpError::Config(format!(
                "unknown model '{model_value}' for session '{session_id}'"
            )));
        }
        let model = model_value
            .split_once('/')
            .map(|(_, model)| model)
            .unwrap_or(model_value);
        let config = runtime.provider.provider_config_mut();
        let previous_model = config.model.clone();
        config.model = model.to_string();
        let dyn_model = match runtime.provider.build_dyn_model().await {
            Ok(dyn_model) => dyn_model,
            Err(e) => {
                runtime.provider.provider_config_mut().model = previous_model;
                return Err(AcpError::Runtime(format!(
                    "failed to build model '{model_value}': {e}"
                )));
            }
        };
        *runtime.dyn_model.lock().unwrap() = dyn_model;
        let mut options = model_options(&runtime, self.models_cache.as_deref());
        options.extend(agent_options(&runtime));
        Ok(options)
    }

    /// Switch `session_id` to `agent_name` and return the updated config options.
    ///
    /// `agent_name` must name an available persona; anything else is rejected
    /// with [`AcpError::Config`]. The switch runs the core agent-switch flow, so
    /// the persona body, model, config overrides, permissions overlay, session
    /// grants, and tool definitions all follow the same path as the plugin and
    /// TUI runtimes. Returns [`AcpError::SessionNotFound`] when no live session
    /// has that id.
    pub async fn set_agent(
        &self,
        session_id: &str,
        agent_name: &str,
    ) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let mut runtime = session.runtime.lock().await;
        runtime
            .switch_agent(agent_name)
            .await
            .map_err(AcpError::Config)?;
        let mut options = agent_options(&runtime);
        options.extend(model_options(&runtime, self.models_cache.as_deref()));
        Ok(options)
    }

    /// Run one prompt turn on `session_id` and report why it stopped.
    ///
    /// The turn runs concurrently with the bus drain loop, so every
    /// `session/update` reaches `sink` as it is published and every permission
    /// request reaches the bridge. The bridge is built here from the pending
    /// map installed on the session runtime, so it resolves the same oneshots
    /// the turn's permission resolver registers. A cancel requested while the
    /// turn runs wins over the turn's own result: ACP requires
    /// `stopReason: cancelled` for every `session/cancel`.
    ///
    /// The registry lock is held only long enough to clone the session handle;
    /// the turn then runs under the session's own runtime lock, so
    /// [`Self::cancel`] can interleave.
    pub async fn prompt<S>(
        &self,
        session_id: &str,
        prompt_text: String,
        sink: &S,
    ) -> Result<StopReason>
    where
        S: SessionUpdateSink + PermissionRequestSink,
    {
        let session = self.session(session_id)?;
        session.cancel_requested.store(false, Ordering::SeqCst);

        let mut rx = session.bus.ui_event().subscribe();
        let mut translator = EventTranslator::new(session_id);
        let result = {
            let mut runtime = session.runtime.lock().await;
            let pending = runtime
                .interactive_pending_arc()
                .unwrap_or_else(|| Arc::new(StdMutex::new(HashMap::new())));
            let mut bridge = AcpPermissionBridge::new(sink, session_id, pending);
            // Control-plane turn-start event: stays on `bus.turn()`, not
            // rendered. No A2A task id exists on the ACP path, so the
            // attribution is always `None`.
            let _ = session
                .bus
                .turn()
                .send(TurnEvent::Started {
                    prompt: prompt_text.clone(),
                    task_id: None,
                })
                .await;
            let turn = runtime.execute_turn(&session.bus, prompt_text, None, Span::unknown());
            run_turn_with_notifications(&mut rx, sink, &mut translator, &mut bridge, turn).await
        };

        let outcome = if session.cancel_requested.load(Ordering::SeqCst) {
            TurnOutcome::Cancelled
        } else {
            classify_turn_result(session_id, result)
        };
        Ok(map_turn_outcome_to_stop_reason(outcome))
    }

    /// Ask the session's turn to stop and report `stopReason: cancelled`.
    ///
    /// Never takes the session's runtime lock, so it completes while a turn is
    /// in flight. Returns [`AcpError::SessionNotFound`] when no live session has
    /// that id.
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let session = self.session(session_id)?;
        session.cancel_requested.store(true, Ordering::SeqCst);
        match session.bus.cancel().request_cancel().await {
            // No turn is listening yet: the flag alone carries the cancel into
            // the next turn, so this is a no-op rather than a failure.
            Ok(()) | Err(ChannelError::NoReceiver) => Ok(()),
            Err(err) => Err(AcpError::Runtime(format!(
                "failed to cancel session '{session_id}': {err}"
            ))),
        }
    }

    /// The live session with `id`, if any.
    pub fn get_session(&self, id: &str) -> Option<Arc<AcpSession>> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    /// Drop the live session with `id`, freeing its runtime and closing its bus.
    ///
    /// The session stays in the store: closing is not deleting. Returns
    /// [`AcpError::SessionNotFound`] when no live session has that id.
    ///
    /// Publishes [`SessionEvent::Ended`] on the session's bus before the bus
    /// drops, so a session-lifecycle hook sees the close.
    pub async fn close_session(&self, id: &str) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| AcpError::SessionNotFound(id.to_string()))?;
        let _ = session
            .bus()
            .session()
            .send(SessionEvent::Ended {
                session_id: id.to_string(),
            })
            .await;
        Ok(())
    }

    /// Drop the live session with `id` and delete it from the store.
    ///
    /// The session's rig-memory store is deleted with it, so no log is left
    /// orphaned. Idempotent: deleting an id that is neither live nor persisted
    /// succeeds.
    ///
    /// Publishes [`SessionEvent::Ended`] on the session's bus when a live
    /// session was removed; a session that was never live has no bus to
    /// publish on.
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let live = self.sessions.lock().unwrap().remove(id);
        if let Some(session) = &live {
            let _ = session
                .bus()
                .session()
                .send(SessionEvent::Ended {
                    session_id: id.to_string(),
                })
                .await;
        }
        let rig = self
            .rig
            .open(id)
            .map_err(|e| AcpError::Runtime(format!("failed to open session '{id}': {e}")))?;
        rig.destroy().map_err(|e| {
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
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|session| SessionInfo {
                id: session.id.clone(),
                message_count: 0,
                last_active: session.created_at,
                title: None,
            })
            .collect();
        sessions.sort_by_key(|info| std::cmp::Reverse(info.last_active));
        sessions
    }

    /// The live session handle for `session_id`, cloned out of the registry.
    ///
    /// The registry lock is released before this returns, so the caller can
    /// hold the session for as long as it likes without blocking the registry.
    fn session(&self, session_id: &str) -> Result<Arc<AcpSession>> {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| AcpError::SessionNotFound(session_id.to_string()))
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
        let client_fs = self.client_fs.lock().unwrap().clone();
        let runtime = self
            .runtime_builder
            .build(AcpRuntimeParams {
                cwd: cwd.clone(),
                mcp_servers,
                config_path: self.config_path.clone(),
                runtime_handle: self.runtime_handle.clone(),
                store_type: self.rig.store_type(),
                session_id: Some(session_id.to_string()),
                client_fs,
                agent_name: self.agent_name.clone(),
            })
            .await?;
        let bus = runtime.bus.clone();
        self.sessions.lock().unwrap().insert(
            session_id.to_string(),
            Arc::new(AcpSession {
                id: session_id.to_string(),
                cwd: cwd.clone(),
                created_at: Utc::now(),
                cancel_requested: Arc::new(AtomicBool::new(false)),
                bus: bus.clone(),
                runtime: Arc::new(AsyncMutex::new(runtime)),
            }),
        );
        // Each session owns its bus, so each session starts its own hook
        // runner. Zero hooks spawns nothing.
        if !self.hooks.is_empty() {
            let _handles = agnt_core::hooks::spawn_hook_runner(
                &bus,
                &self.hooks,
                &cwd,
                agnt_core::hooks::NuProcess,
            );
        }
        // `hydrated` reports whether the store already holds this session's
        // log, so a session-lifecycle hook can tell a fresh session from a
        // resumed one. A store that cannot be read is treated as not hydrated.
        let hydrated = self.rig.holds(session_id).unwrap_or(false);
        let _ = bus
            .session()
            .send(SessionEvent::Started {
                session_id: session_id.to_string(),
                hydrated,
            })
            .await;
        Ok(())
    }

    /// The stored entries of `session_id`, or [`AcpError::SessionNotFound`].
    ///
    /// The conversation lives in the rig-memory store the agent writes; the
    /// session store is not consulted. `rig_memory::load` returns an empty
    /// conversation for a log that does not exist, so absence is detected from
    /// the store's log names first.
    async fn load_entries(&self, session_id: &str) -> Result<Vec<Message>> {
        let store = self.rig.open(session_id).map_err(|e| {
            AcpError::Runtime(format!("failed to open session '{session_id}': {e}"))
        })?;
        let logs = store.logs().map_err(|e| {
            AcpError::Runtime(format!("failed to list session logs '{session_id}': {e}"))
        })?;
        if !logs.iter().any(|log| log == session_id) {
            return Err(AcpError::SessionNotFound(session_id.to_string()));
        }
        let conversation = rig_memory::load(&*store, session_id).map_err(|e| {
            AcpError::Runtime(format!("failed to load session '{session_id}': {e}"))
        })?;
        Ok(conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect())
    }
}

// endregion: --- AcpSessionManager Impl

// region:    --- Support

/// The agent config options of a locked session runtime.
fn agent_options(runtime: &AgentConversationRuntime) -> Vec<SessionConfigOption> {
    build_agent_config_options(
        runtime.multi_agent.agents_config(),
        &runtime.cwd,
        runtime.persona.agent_identity(),
    )
}

/// The model config options of a locked session runtime.
fn model_options(
    runtime: &AgentConversationRuntime,
    models_cache: Option<&ModelsCache>,
) -> Vec<SessionConfigOption> {
    build_model_config_options(runtime.provider.provider_config(), models_cache)
}

/// Replay the stored conversation of `session_id` to `sink`.
///
/// Only the conversational text is replayed: user turns become
/// `user_message_chunk` and assistant turns become `agent_message_chunk`.
/// Tool calls, tool results, and compaction summaries have no replay form and
/// are skipped. Returns the transport error when the sink is gone.
fn replay_history<S: SessionUpdateSink>(
    sink: &S,
    session_id: &str,
    messages: &[Message],
) -> Result<()> {
    for message in messages {
        match message {
            Message::User { content } => {
                for item in content.iter() {
                    if let UserContent::Text(text) = item {
                        send_chunk(sink, session_id, text.text.clone(), true)?;
                    }
                }
            }
            Message::Assistant(assistant) => {
                for item in assistant.content.iter() {
                    if let AssistantContent::Text(text) = item {
                        send_chunk(sink, session_id, text.text.clone(), false)?;
                    }
                }
            }
            Message::System { .. } => {}
        }
    }
    Ok(())
}

/// Send one replayed message as a `session/update` notification.
fn send_chunk<S: SessionUpdateSink>(
    sink: &S,
    session_id: &str,
    text: String,
    from_user: bool,
) -> Result<()> {
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
    let update = if from_user {
        SessionUpdate::UserMessageChunk(chunk)
    } else {
        SessionUpdate::AgentMessageChunk(chunk)
    };
    sink.send_update(session_id, update)
        .map_err(|e| AcpError::Runtime(format!("failed to replay session '{session_id}': {e}")))
}

/// Every selectable value of the select options in `options`.
///
/// Non-select options and unknown option kinds contribute no values, so a
/// caller can validate a requested value against the advertised set.
fn select_values(options: &[SessionConfigOption]) -> Vec<String> {
    options
        .iter()
        .flat_map(|option| match &option.kind {
            SessionConfigKind::Select(select) => match &select.options {
                SessionConfigSelectOptions::Ungrouped(values) => {
                    values.iter().map(|value| value.value.to_string()).collect()
                }
                SessionConfigSelectOptions::Grouped(groups) => groups
                    .iter()
                    .flat_map(|group| group.options.iter())
                    .map(|value| value.value.to_string())
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

/// Classify a finished turn that the client did not cancel.
///
/// `CoreRuntime::execute_turn` erases its structured `TurnError` into a
/// `LabeledError`, so the message is the only signal left at this boundary.
/// Max-turn and max-token stops are recognised by their message markers;
/// everything else counts as a completed turn. A failed turn is logged before
/// it is mapped, so a swallowed error is never silent.
fn classify_turn_result(
    session_id: &str,
    result: core::result::Result<Value, LabeledError>,
) -> TurnOutcome {
    let Err(err) = result else {
        return TurnOutcome::Completed;
    };
    log::warn!("session {session_id}: prompt turn failed: {}", err.msg);
    let message = err.msg.to_lowercase();
    if message.contains("max turns") {
        TurnOutcome::MaxTurns
    } else if message.contains("max tokens") {
        TurnOutcome::MaxTokens
    } else {
        TurnOutcome::Completed
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/session.rs"]
mod session_test;

// endregion: --- Tests

//! Unit tests for the ACP session manager.
//!
//! Every test drives the public `AcpSessionManager` API against an in-memory
//! session store, so nothing touches the network or the filesystem store.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    ContentBlock, FileSystemCapabilities, LoadSessionResponse, PermissionOption,
    RequestPermissionOutcome, SessionConfigKind, SessionConfigOptionValue,
    SessionConfigSelectOptions, SessionUpdate, SetSessionConfigOptionRequest, StopReason,
    ToolCallUpdate,
};
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
use agnt_core::bus::{CancelEvent, OneshotTx, create_bus};
use agnt_core::compaction::CompactionParams;
use agnt_core::config::{AgentsConfig, Config, ModelsCache, defaults};
use agnt_core::conversation::compaction::CompactionConfig;
use agnt_core::conversation::compaction::compactor::NuCompactor;
use agnt_core::conversation::managers::ProviderManager;
use agnt_core::conversation::providers::CachedProviderClient;
use agnt_core::conversation::state::{
    McpState, MemoryState, MultiAgentState, PermissionState, PersonaState, ProviderState, ToolState,
};
use agnt_core::hook::DoomLoopState;
use agnt_core::hook::output_repetition::RepetitionState;
use agnt_core::protocol::event::PermissionDecision;
use agnt_core::session::factory::create_store_in;
use agnt_core::session::{SessionStore, SessionStoreBackend, StoreType};
use agnt_core::tools::authz::{PermissionsConfig, SessionGrantCache};
use agnt_core::tools::closure::ClosureRegistry;
use agnt_core::tools::handler::McpToolRegistry;
use agnt_core::tools::handler::builtin_tool::ToolRenderRegistry;
use agnt_core::tools::mcp::circuit_breaker::McpCircuitBreaker;
use agnt_core::types::{Message, MockCompletionModel};

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Test double for [`RuntimeBuilder`]: builds a runtime with a mock model.
///
/// The runtime is built without config.toml, without a `reqwest::Client`, and
/// without TLS initialization, so session tests stay fast. When
/// `captured_client_fs` is set, every build records the `client_fs` it received.
#[derive(Default)]
struct MockRuntimeBuilder {
    captured_client_fs: Option<Arc<Mutex<Vec<Option<ClientFsState>>>>>,
}

impl RuntimeBuilder for MockRuntimeBuilder {
    async fn build(
        &self,
        params: AcpRuntimeParams,
    ) -> crate::error::Result<AgentConversationRuntime> {
        let AcpRuntimeParams {
            cwd,
            mcp_servers: _,
            config_path: _,
            runtime_handle,
            store,
            session_id,
            client_fs,
        } = params;
        if let Some(captured) = &self.captured_client_fs {
            captured.lock().unwrap().push(client_fs);
        }

        let bus = create_bus();
        let model = MockCompletionModel::text("mock response");
        let compactor_model = model.clone();
        let config = Config {
            provider: "ollama".to_string(),
            model: "llama3".to_string(),
            ..Config::default()
        };
        let mut provider = ProviderState::with_client(config, CachedProviderClient::Mock(model));
        let dyn_model = provider
            .build_dyn_model()
            .map_err(|e| AcpError::Runtime(format!("failed to build mock model: {e}")))?;
        let compactor = NuCompactor::from_shared_model(
            Arc::new(Mutex::new(compactor_model.erase())),
            bus.clone(),
            None,
        )
        .with_store(Arc::clone(&store));

        Ok(AgentConversationRuntime {
            runtime: runtime_handle,
            tool_server_handle: agnt_core::types::ToolServer::new().run(),
            provider,
            tools: ToolState::new(Vec::new(), Vec::new(), ClosureRegistry::default()),
            mcp_state: McpState::new(
                None,
                Vec::new(),
                Vec::new(),
                Some(cwd.clone()),
                McpToolRegistry::empty(),
                defaults::MAX_TOOL_RESULT_BYTES,
                None,
            ),
            engine: Arc::new(crate::AcpEngineStub::new(cwd.to_string_lossy().to_string())),
            store: Arc::clone(&store),
            final_session_id: session_id,
            permission_state: PermissionState::new(
                PermissionsConfig::safe_defaults(true),
                PermissionsConfig::safe_defaults(true),
                None,
                SessionGrantCache::default(),
                String::new(),
            ),
            session: MemoryState::new(Arc::clone(&store)),
            persona: PersonaState::new(None, None, None, None, None, None, None),
            multi_agent: MultiAgentState::new(Vec::new(), AgentsConfig::default()),
            cwd,
            interactive_pending: Some(Arc::new(Mutex::new(HashMap::new()))),
            renders_tool_previews: false,
            circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: defaults::REPETITION_GUARD_ENABLED,
            last_total_tokens: Arc::new(Mutex::new(None)),
            bus,
            render_registry: ToolRenderRegistry::default(),
            dyn_model: Arc::new(Mutex::new(dyn_model)),
            compaction: CompactionConfig {
                compactor,
                params: CompactionParams::default(),
                threshold_tokens: None,
            },
        })
    }
}

/// Test double for [`RuntimeBuilder`] that always fails.
///
/// Drives the manager's error path without building a real runtime.
struct FailingRuntimeBuilder;

impl RuntimeBuilder for FailingRuntimeBuilder {
    async fn build(
        &self,
        _params: AcpRuntimeParams,
    ) -> crate::error::Result<AgentConversationRuntime> {
        Err(AcpError::Config("no runtime for this test".to_string()))
    }
}

async fn memory_store(dir: &Path) -> Result<Arc<SessionStoreBackend>> {
    let store = create_store_in(StoreType::Memory, dir)
        .await
        .map_err(|e| format!("should create in-memory store: {e:?}"))?;
    Ok(Arc::new(store))
}

async fn manager(dir: &Path) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    let store = memory_store(dir).await?;
    Ok(AcpSessionManager::with_runtime_builder(
        store,
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    ))
}

fn session_cwd(dir: &Path, name: &str) -> Result<PathBuf> {
    let cwd = dir.join(name);
    std::fs::create_dir_all(&cwd)?;
    Ok(cwd)
}

/// A manager plus the store it shares, so a test can seed persisted sessions.
async fn manager_with_store(
    dir: &Path,
) -> Result<(
    AcpSessionManager<MockRuntimeBuilder>,
    Arc<SessionStoreBackend>,
)> {
    let store = memory_store(dir).await?;
    let manager = AcpSessionManager::with_runtime_builder(
        Arc::clone(&store),
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    );
    Ok((manager, store))
}

/// Persist a two-message conversation under `session_id`.
async fn seed_session(store: &SessionStoreBackend, session_id: &str) -> Result<()> {
    store
        .create(
            session_id,
            &[
                Message::user("hello".to_string()),
                Message::assistant("hi there".to_string()),
            ],
        )
        .await
        .map_err(|e| format!("should seed session: {e:?}"))?;
    Ok(())
}

/// A live in-memory `ConnectionTo<Client>` for tests that need a client handle.
///
/// The connection runs over an in-memory channel: no transport, no network, no
/// subprocess. The returned peer keeps the channel open for the test's lifetime.
async fn client_connection() -> Result<(ConnectionTo<Client>, Channel)> {
    let (channel, peer) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;
    Ok((connection, peer))
}

/// A manager whose runtime builder records the `client_fs` of every build.
async fn manager_capturing_client_fs(
    dir: &Path,
) -> Result<(
    AcpSessionManager<MockRuntimeBuilder>,
    Arc<Mutex<Vec<Option<ClientFsState>>>>,
)> {
    let store = memory_store(dir).await?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        store,
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            captured_client_fs: Some(Arc::clone(&captured)),
        },
    );
    Ok((manager, captured))
}

/// Test double for [`SessionUpdateSink`]: records every notification.
#[derive(Default)]
struct RecordingSink {
    updates: Mutex<Vec<SessionUpdate>>,
    fail: bool,
}

impl RecordingSink {
    fn failing() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            fail: true,
        }
    }

    fn taken(&self) -> Vec<SessionUpdate> {
        self.updates
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl SessionUpdateSink for RecordingSink {
    fn send_update(
        &self,
        _session_id: &str,
        update: SessionUpdate,
    ) -> core::result::Result<(), AcpError> {
        if self.fail {
            return Err(AcpError::Runtime("transport closed".to_string()));
        }
        if let Ok(mut guard) = self.updates.lock() {
            guard.push(update);
        }
        Ok(())
    }
}

/// The text of a replayed message chunk, or `None` for any other update.
fn chunk_text(update: &SessionUpdate) -> Option<String> {
    let chunk = match update {
        SessionUpdate::UserMessageChunk(chunk) | SessionUpdate::AgentMessageChunk(chunk) => chunk,
        _ => return None,
    };
    match &chunk.content {
        ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    }
}

/// The prompt tests never reach a tool call, so this sink is never asked; it
/// answers `Cancelled` if it ever is.
impl PermissionRequestSink for RecordingSink {
    async fn send_request(
        &self,
        _session_id: &str,
        _tool_call: ToolCallUpdate,
        _options: Vec<PermissionOption>,
    ) -> RequestPermissionOutcome {
        RequestPermissionOutcome::Cancelled
    }
}

// ---------------------------------------------------------------------------
// new_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_new_session_returns_non_empty_id() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    assert!(!session_id.is_empty(), "session id must not be empty");
    Ok(())
}

#[tokio::test]
async fn test_session_manager_new_session_registers_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd.clone(), Vec::new()).await?;
    let session = manager
        .get_session(&session_id)
        .ok_or("a new session must be retrievable")?;

    // -- Check
    assert_eq!(session.id(), session_id);
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_new_session_builder_error_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store = memory_store(temp.path()).await?;
    let manager = AcpSessionManager::with_runtime_builder(
        store,
        temp.path().join("config.toml"),
        tokio::runtime::Handle::current(),
        FailingRuntimeBuilder,
    );

    // -- Exec
    let result = manager
        .new_session(temp.path().to_path_buf(), Vec::new())
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("a failing runtime builder must fail session creation")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "the builder error must propagate, got: {err:?}"
    );
    assert!(
        manager.list_sessions().is_empty(),
        "a session that failed to build must not be registered"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// client filesystem state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_open_session_passes_client_fs_when_initialized() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_client_fs(temp.path()).await?;
    let (connection, _peer) = client_connection().await?;
    let capabilities = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let state = Arc::new(Mutex::new(Some(ClientFsState {
        connection,
        capabilities: capabilities.clone(),
    })));
    let manager = manager.with_client_fs(state);
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let passed = captured
        .first()
        .ok_or("the runtime builder must be called once")?
        .clone()
        .ok_or("open_session must pass Some(ClientFsState) when initialize stored one")?;
    assert_eq!(passed.capabilities, capabilities);
    Ok(())
}

#[tokio::test]
async fn test_session_manager_open_session_passes_none_client_fs_without_initialize() -> Result<()>
{
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, captured) = manager_capturing_client_fs(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    manager.new_session(cwd, Vec::new()).await?;

    // -- Check
    let captured = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let passed = captured
        .first()
        .ok_or("the runtime builder must be called once")?;
    assert!(
        passed.is_none(),
        "open_session must pass None when no initialize was received"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// get_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_get_session_unknown_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let session = manager.get_session("no-such-session");

    // -- Check
    assert!(session.is_none(), "an unknown id must not resolve");
    Ok(())
}

// ---------------------------------------------------------------------------
// list_sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_list_sessions_includes_new_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert!(
        listed.iter().any(|info| info.id == session_id),
        "list_sessions must include the new session; got: {listed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_sessions_are_independent() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let first_cwd = session_cwd(temp.path(), "first-cwd")?;
    let second_cwd = session_cwd(temp.path(), "second-cwd")?;

    // -- Exec
    let first = manager.new_session(first_cwd.clone(), Vec::new()).await?;
    let second = manager.new_session(second_cwd.clone(), Vec::new()).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert_ne!(first, second, "each session must get its own id");
    assert_eq!(listed.len(), 2, "both sessions must be listed");
    let first_session = manager
        .get_session(&first)
        .ok_or("first session must exist")?;
    let second_session = manager
        .get_session(&second)
        .ok_or("second session must exist")?;
    assert_eq!(first_session.cwd(), first_cwd.as_path());
    assert_eq!(second_session.cwd(), second_cwd.as_path());
    Ok(())
}

// ---------------------------------------------------------------------------
// close_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_close_session_frees_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.close_session(&session_id)?;

    // -- Check
    assert!(
        manager.get_session(&session_id).is_none(),
        "a closed session must be freed"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_close_session_unknown_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.close_session("no-such-session");

    // -- Check
    let err = result.err().ok_or("closing an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// delete_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_delete_session_removes_from_list() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.delete_session(&session_id).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert!(
        !listed.iter().any(|info| info.id == session_id),
        "a deleted session must not be listed; got: {listed:?}"
    );
    assert!(
        manager.get_session(&session_id).is_none(),
        "a deleted session must be freed"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_delete_session_keeps_other_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let first_cwd = session_cwd(temp.path(), "first-cwd")?;
    let second_cwd = session_cwd(temp.path(), "second-cwd")?;
    let first = manager.new_session(first_cwd, Vec::new()).await?;
    let second = manager.new_session(second_cwd, Vec::new()).await?;

    // -- Exec
    manager.delete_session(&first).await?;
    let listed = manager.list_sessions();

    // -- Check
    assert_eq!(listed.len(), 1, "only the deleted session must be gone");
    assert_eq!(listed[0].id, second);
    Ok(())
}

// ---------------------------------------------------------------------------
// StopReason mapping
// ---------------------------------------------------------------------------

#[test]
fn test_map_turn_outcome_completed_returns_end_turn() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::Completed);

    // -- Check
    assert_eq!(stop_reason, StopReason::EndTurn);
}

#[test]
fn test_map_turn_outcome_cancelled_returns_cancelled() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::Cancelled);

    // -- Check
    assert_eq!(stop_reason, StopReason::Cancelled);
}

#[test]
fn test_map_turn_outcome_max_tokens_returns_max_tokens() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::MaxTokens);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTokens);
}

#[test]
fn test_map_turn_outcome_max_turns_returns_max_turn_requests() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::MaxTurns);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTurnRequests);
}

// ---------------------------------------------------------------------------
// cancel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_cancel_signals_the_session_bus() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let bus = manager
        .get_session(&session_id)
        .ok_or("session must exist")?
        .bus()
        .clone();
    let mut cancel_rx = bus.cancel().subscribe();

    // -- Exec
    manager.cancel(&session_id).await?;
    let event = cancel_rx.recv().await?;

    // -- Check
    assert!(
        matches!(event, CancelEvent::Requested),
        "cancel must publish CancelEvent::Requested on the session bus"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_cancel_unknown_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.cancel("no-such-session").await;

    // -- Check
    let err = result
        .err()
        .ok_or("cancelling an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// load_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_load_session_replays_history() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::default();

    // -- Exec
    let response = manager.load_session("sess-1", cwd, &sink).await?;

    // -- Check
    let updates = sink.taken();
    assert_eq!(
        updates.len(),
        2,
        "both stored messages must be replayed; got: {updates:?}"
    );
    assert!(
        matches!(updates[0], SessionUpdate::UserMessageChunk(_)),
        "the user turn must replay as user_message_chunk; got: {:?}",
        updates[0]
    );
    assert!(
        matches!(updates[1], SessionUpdate::AgentMessageChunk(_)),
        "the assistant turn must replay as agent_message_chunk; got: {:?}",
        updates[1]
    );
    assert_eq!(chunk_text(&updates[0]).as_deref(), Some("hello"));
    assert_eq!(chunk_text(&updates[1]).as_deref(), Some("hi there"));
    assert_eq!(response, LoadSessionResponse::new());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_registers_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::default();

    // -- Exec
    manager.load_session("sess-1", cwd.clone(), &sink).await?;
    let session = manager
        .get_session("sess-1")
        .ok_or("a loaded session must be live")?;

    // -- Check
    assert_eq!(session.id(), "sess-1");
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_missing_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, _store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let sink = RecordingSink::default();

    // -- Exec
    let result = manager.load_session("no-such-session", cwd, &sink).await;

    // -- Check
    let err = result.err().ok_or("loading a missing session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    assert!(
        sink.taken().is_empty(),
        "nothing must be replayed for a missing session"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_load_session_failing_sink_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;
    let sink = RecordingSink::failing();

    // -- Exec
    let result = manager.load_session("sess-1", cwd, &sink).await;

    // -- Check
    let err = result
        .err()
        .ok_or("a dead transport must fail the replay")?;
    assert!(
        matches!(err, AcpError::Runtime(_)),
        "a dead transport must produce AcpError::Runtime, got: {err:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// resume_session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_resume_session_returns_id() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    seed_session(&store, "sess-1").await?;

    // -- Exec
    let session_id = manager.resume_session("sess-1", cwd.clone()).await?;
    let session = manager
        .get_session(&session_id)
        .ok_or("a resumed session must be live")?;

    // -- Check
    assert_eq!(session_id, "sess-1");
    assert_eq!(session.cwd(), cwd.as_path());
    Ok(())
}

#[tokio::test]
async fn test_session_manager_resume_session_missing_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let (manager, _store) = manager_with_store(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;

    // -- Exec
    let result = manager.resume_session("no-such-session", cwd).await;

    // -- Check
    let err = result.err().ok_or("resuming a missing session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    assert!(
        manager.get_session("no-such-session").is_none(),
        "a failed resume must not register a session"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Concurrent prompt + cancel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_cancel_during_prompt_returns_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // Park the turn on the session's runtime lock: the prompt is then in flight
    // but has not started its turn, which is the state a real turn spends its
    // whole life in.
    let runtime = Arc::clone(
        manager
            .get_session(&session_id)
            .ok_or("session must exist")?
            .runtime(),
    );
    let guard = runtime.lock().await;

    // -- Exec
    let prompt = manager.prompt(&session_id, "hello".to_string(), &sink);
    tokio::pin!(prompt);
    let parked = tokio::time::timeout(Duration::from_millis(50), &mut prompt).await;
    let cancel = manager.cancel(&session_id).await;
    drop(guard);
    let stop_reason = prompt.await?;

    // -- Check
    assert!(
        parked.is_err(),
        "the prompt must park on the runtime lock while the turn is in flight"
    );
    assert!(
        cancel.is_ok(),
        "cancel must complete while the turn holds the runtime lock"
    );
    assert_eq!(stop_reason, StopReason::Cancelled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt permission bridge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_manager_prompt_bridge_shares_runtime_pending_map() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // Register a decision in the runtime's pending map. The bridge that
    // `prompt()` builds must drain this same map when the turn ends.
    let pending = {
        let session = manager
            .get_session(&session_id)
            .ok_or("session must exist")?;
        let runtime = session.runtime().lock().await;
        runtime
            .interactive_pending_arc()
            .ok_or("the ACP runtime must install the interactive pending map")?
    };
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("test-permission");
    pending
        .lock()
        .map_err(|_| "pending map must not be poisoned")?
        .insert("req-1".to_string(), tx);

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await;
    let decision = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .map_err(|_| "the bridge must drain the runtime pending map")??;

    // -- Check
    assert_eq!(
        decision,
        PermissionDecision::Deny,
        "an unresolved request must be denied when the turn ends"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Model config options
// ---------------------------------------------------------------------------

/// A models cache holding `models` under `provider`.
fn models_cache(provider: &str, models: &[(&str, &str)]) -> Result<ModelsCache> {
    let models: serde_json::Map<String, serde_json::Value> = models
        .iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "tool_call": true,
                    "limit": { "context": 8192, "output": 4096 },
                }),
            )
        })
        .collect();
    let json = serde_json::json!({
        (provider): {
            "id": provider,
            "name": provider,
            "models": models,
        }
    });
    Ok(serde_json::from_value(json)?)
}

/// A manager that advertises `cache` as its models cache.
async fn manager_with_cache(
    dir: &Path,
    cache: ModelsCache,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    let store = memory_store(dir).await?;
    Ok(AcpSessionManager::with_runtime_builder(
        store,
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    )
    .with_models_cache(Some(Arc::new(cache))))
}

/// The current value of the first option.
fn current_value(options: &[SessionConfigOption]) -> Option<String> {
    let option = options.first()?;
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

/// The selectable values of the first option.
fn select_values(options: &[SessionConfigOption]) -> Vec<String> {
    let Some(option) = options.first() else {
        return Vec::new();
    };
    let SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(values) => {
            values.iter().map(|value| value.value.to_string()).collect()
        }
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|value| value.value.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

#[tokio::test]
async fn test_session_manager_model_config_options_without_cache_lists_current() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.model_config_options(&session_id).await?;

    // -- Check
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama3"));
    assert_eq!(select_values(&options), vec!["ollama/llama3".to_string()]);
    Ok(())
}

#[tokio::test]
async fn test_session_manager_model_config_options_unknown_session_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.model_config_options("no-such-session").await;

    // -- Check
    let err = result.err().ok_or("an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_valid_updates_current_value() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("ollama", &[("llama3", "Llama 3"), ("llama4", "Llama 4")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.set_model(&session_id, "ollama/llama4").await?;

    // -- Check
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama4"));
    let refreshed = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&refreshed).as_deref(),
        Some("ollama/llama4"),
        "the session must keep the switched model"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_unknown_value_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("ollama", &[("llama3", "Llama 3")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let result = manager.set_model(&session_id, "ollama/not-a-model").await;

    // -- Check
    let err = result
        .err()
        .ok_or("an unadvertised model must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unadvertised model must produce AcpError::Config, got: {err:?}"
    );
    let options = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&options).as_deref(),
        Some("ollama/llama3"),
        "a rejected switch must leave the model unchanged"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_unknown_session_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.set_model("no-such-session", "ollama/llama3").await;

    // -- Check
    let err = result.err().ok_or("an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_build_failure_restores_config() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("bad-provider", &[("target-model", "Target Model")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // Sabotage the provider: switch to an unsupported name and drop the cached
    // client, so the next build_dyn_model() rebuilds and fails.
    {
        let session = manager
            .get_session(&session_id)
            .ok_or("session must exist")?;
        let mut runtime = session.runtime().lock().await;
        runtime.provider.invalidate_cache();
        let config = runtime.provider.provider_config_mut();
        config.provider = "bad-provider".to_string();
        config.model = "initial".to_string();
    }

    // -- Exec
    let result = manager
        .set_model(&session_id, "bad-provider/target-model")
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("set_model must fail when build_dyn_model fails")?;
    assert!(
        matches!(err, AcpError::Runtime(_)),
        "build_dyn_model failure must produce AcpError::Runtime, got: {err:?}"
    );
    let options = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&options).as_deref(),
        Some("bad-provider/initial"),
        "config.model must be restored to 'initial' after build_dyn_model failure"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/set_config_option handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_set_config_option_valid_value_returns_updated_options() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("ollama", &[("llama3", "Llama 3"), ("llama4", "Llama 4")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request =
        SetSessionConfigOptionRequest::new(session_id, crate::MODEL_CONFIG_ID, "ollama/llama4");

    // -- Exec
    let response = crate::handle_set_config_option(&manager, request).await?;

    // -- Check
    assert_eq!(
        current_value(&response.config_options).as_deref(),
        Some("ollama/llama4"),
        "the response must carry the updated current value"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_set_config_option_unknown_config_id_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request = SetSessionConfigOptionRequest::new(session_id, "mode", "ollama/llama3");

    // -- Exec
    let result = crate::handle_set_config_option(&manager, request).await;

    // -- Check
    let err = result
        .err()
        .ok_or("an unknown config id must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unknown config id must produce AcpError::Config, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_set_config_option_non_id_value_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request = SetSessionConfigOptionRequest::new(
        session_id,
        crate::MODEL_CONFIG_ID,
        SessionConfigOptionValue::boolean(true),
    );

    // -- Exec
    let result = crate::handle_set_config_option(&manager, request).await;

    // -- Check
    let err = result.err().ok_or("a non-id value must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a non-id value must produce AcpError::Config, got: {err:?}"
    );
    Ok(())
}

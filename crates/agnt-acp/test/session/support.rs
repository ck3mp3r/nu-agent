//! Shared fixtures for the ACP session manager test modules.
//!
//! Holds the imports and the helpers every topical session test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use std::collections::HashMap;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::sync::{Arc, Mutex};
pub(super) use std::time::Duration;

pub(super) use agent_client_protocol::schema::v1::{
    ContentBlock, FileSystemCapabilities, LoadSessionResponse, PermissionOption,
    RequestPermissionOutcome, SessionConfigKind, SessionConfigOptionValue,
    SessionConfigSelectOptions, SessionUpdate, SetSessionConfigOptionRequest, StopReason,
    ToolCallUpdate,
};
pub(super) use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
pub(super) use agnt_core::bus::{CancelEvent, OneshotTx, create_bus};
pub(super) use agnt_core::compaction::CompactionParams;
pub(super) use agnt_core::config::{AgentsConfig, Config, ModelsCache, defaults};
pub(super) use agnt_core::conversation::compaction::CompactionConfig;
pub(super) use agnt_core::conversation::compaction::compactor::NuCompactor;
pub(super) use agnt_core::conversation::managers::ProviderManager;
pub(super) use agnt_core::conversation::providers::CachedProviderClient;
pub(super) use agnt_core::conversation::state::{
    McpState, MemoryState, MultiAgentState, PermissionState, PersonaState, ProviderState, ToolState,
};
pub(super) use agnt_core::hook::DoomLoopState;
pub(super) use agnt_core::hook::output_repetition::RepetitionState;
pub(super) use agnt_core::protocol::event::PermissionDecision;
pub(super) use agnt_core::session::factory::create_store_in;
pub(super) use agnt_core::session::{SessionStore, SessionStoreBackend, StoreType};
pub(super) use agnt_core::tools::authz::{PermissionsConfig, SessionGrantCache};
pub(super) use agnt_core::tools::closure::ClosureRegistry;
pub(super) use agnt_core::tools::handler::McpToolRegistry;
pub(super) use agnt_core::tools::handler::builtin_tool::ToolRenderRegistry;
pub(super) use agnt_core::tools::mcp::circuit_breaker::McpCircuitBreaker;
pub(super) use agnt_core::types::{Message, MockCompletionModel};

use super::super::*;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Test double for [`RuntimeBuilder`]: builds a runtime with a mock model.
///
/// The runtime is built without config.toml, without a `reqwest::Client`, and
/// without TLS initialization, so session tests stay fast. When
/// `captured_client_fs` is set, every build records the `client_fs` it received.
/// When `captured_agent_name` is set, every build records the `agent_name` it
/// received.
#[derive(Default)]
pub(super) struct MockRuntimeBuilder {
    captured_client_fs: Option<Arc<Mutex<Vec<Option<ClientFsState>>>>>,
    captured_agent_name: Option<Arc<Mutex<Vec<Option<String>>>>>,
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
            agent_name,
        } = params;
        if let Some(captured) = &self.captured_client_fs {
            captured.lock().unwrap().push(client_fs);
        }
        if let Some(captured) = &self.captured_agent_name {
            captured.lock().unwrap().push(agent_name);
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
            .await
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
pub(super) struct FailingRuntimeBuilder;

impl RuntimeBuilder for FailingRuntimeBuilder {
    async fn build(
        &self,
        _params: AcpRuntimeParams,
    ) -> crate::error::Result<AgentConversationRuntime> {
        Err(AcpError::Config("no runtime for this test".to_string()))
    }
}

pub(super) async fn memory_store(dir: &Path) -> Result<Arc<SessionStoreBackend>> {
    let store = create_store_in(StoreType::Memory, dir)
        .await
        .map_err(|e| format!("should create in-memory store: {e:?}"))?;
    Ok(Arc::new(store))
}

pub(super) async fn manager(dir: &Path) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    let store = memory_store(dir).await?;
    Ok(AcpSessionManager::with_runtime_builder(
        store,
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    ))
}

pub(super) fn session_cwd(dir: &Path, name: &str) -> Result<PathBuf> {
    let cwd = dir.join(name);
    std::fs::create_dir_all(&cwd)?;
    Ok(cwd)
}

/// A manager plus the store it shares, so a test can seed persisted sessions.
pub(super) async fn manager_with_store(
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
pub(super) async fn seed_session(store: &SessionStoreBackend, session_id: &str) -> Result<()> {
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
pub(super) async fn client_connection() -> Result<(ConnectionTo<Client>, Channel)> {
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
pub(super) async fn manager_capturing_client_fs(
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
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, captured))
}

/// A manager whose runtime builder records the `agent_name` of every build.
pub(super) async fn manager_capturing_agent_name(
    dir: &Path,
) -> Result<(
    AcpSessionManager<MockRuntimeBuilder>,
    Arc<Mutex<Vec<Option<String>>>>,
)> {
    let store = memory_store(dir).await?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        store,
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            captured_agent_name: Some(Arc::clone(&captured)),
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, captured))
}

/// Test double for [`SessionUpdateSink`]: records every notification.
#[derive(Default)]
pub(super) struct RecordingSink {
    updates: Mutex<Vec<SessionUpdate>>,
    fail: bool,
}

impl RecordingSink {
    pub(super) fn failing() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            fail: true,
        }
    }

    pub(super) fn taken(&self) -> Vec<SessionUpdate> {
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
pub(super) fn chunk_text(update: &SessionUpdate) -> Option<String> {
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

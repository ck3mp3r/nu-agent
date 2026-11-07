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
pub(super) use agnt_core::conversation::contracts::DecisionSource;
pub(super) use agnt_core::conversation::providers::CachedProviderClient;
pub(super) use agnt_core::conversation::runtime_build::RuntimeBuildParams;
pub(super) use agnt_core::conversation::runtime_parts::RuntimeParts;
pub(super) use agnt_core::conversation::state::{
    MultiAgentState, PersonaState, ProviderState, ToolState,
};
pub(super) use agnt_core::types::{Message, MockCompletionModel};
pub(super) use agnt_runtime::bus::{Bus, CancelEvent, OneshotTx, SessionEvent, TurnEvent, TurnRx};
pub(super) use agnt_runtime::config::{AgentsConfig, Config, ModelsCache};
pub(super) use agnt_runtime::conversation::state::memory::MemoryState;
pub(super) use agnt_runtime::protocol::event::PermissionDecision;
pub(super) use agnt_runtime::protocol::events::SystemEvent;
pub(super) use agnt_runtime::session::{RigStoreConfig, StoreType};
pub(super) use agnt_runtime::tools::authz::PermissionsConfig;
pub(super) use agnt_runtime::tools::closure::ClosureRegistry;
pub(super) use agnt_runtime::tools::handler::McpToolRegistry;
pub(super) use rig::test_utils::MockStreamEvent;

use super::super::*;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The context window the mock runtime's active config declares.
///
/// The composed runtime derives the compaction threshold and the compactor's
/// planning spec from it, so the mock scripts and the policy plan against the
/// same window.
const MOCK_CONTEXT_TOKENS: usize = 8_000;

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
pub(super) struct MockRuntimeBuilder {
    captured_client_fs: Option<Arc<Mutex<Vec<Option<ClientFsState>>>>>,
    captured_agent_name: Option<Arc<Mutex<Vec<Option<String>>>>>,
    bus: Option<Bus>,
    /// Usage to seed into the runtime of the session with this id, so a test
    /// can give two sessions distinct cumulative totals.
    usage_for_session: Option<(String, u64, u64)>,
    /// How many streaming turns the mock model scripts. One serves the prompt
    /// turn; a test that also runs a compaction scripts a second for the
    /// summarizer, which shares the model.
    stream_turns: usize,
    /// The chain's auto-compaction threshold. `None` disables the decision.
    threshold_tokens: Option<usize>,
}

impl Default for MockRuntimeBuilder {
    fn default() -> Self {
        Self {
            captured_client_fs: None,
            captured_agent_name: None,
            bus: None,
            usage_for_session: None,
            stream_turns: 1,
            threshold_tokens: None,
        }
    }
}

impl MockRuntimeBuilder {
    /// Seed `input`/`output` tokens into the runtime of `session_id`.
    pub(super) fn with_usage_for_session(
        mut self,
        session_id: &str,
        input: u64,
        output: u64,
    ) -> Self {
        self.usage_for_session = Some((session_id.to_string(), input, output));
        self
    }

    /// Script `turns` streaming turns on the mock model.
    pub(super) fn with_stream_turns(mut self, turns: usize) -> Self {
        self.stream_turns = turns;
        self
    }

    /// Set the chain's auto-compaction threshold, so a turn can trip it.
    pub(super) fn with_threshold_tokens(mut self, tokens: usize) -> Self {
        self.threshold_tokens = Some(tokens);
        self
    }
}

impl RuntimeBuilder for MockRuntimeBuilder {
    async fn build(
        &self,
        params: AcpRuntimeParams,
    ) -> crate::error::Result<AgentConversationRuntime> {
        let AcpRuntimeParams {
            cwd,
            mcp_servers: _,
            paths,
            runtime_handle,
            rig,
            session_id,
            client_fs,
            agent_name,
            hook_context,
        } = params;
        if let Some(captured) = &self.captured_client_fs {
            captured.lock().unwrap().push(client_fs);
        }
        if let Some(captured) = &self.captured_agent_name {
            captured.lock().unwrap().push(agent_name);
        }

        let bus = self.bus.clone().unwrap_or_default();
        // The turn path always streams, so the model must script a streaming
        // turn: a unary-only script fails every prompt turn at the provider.
        // A test that also runs a compaction scripts one extra turn for the
        // summarizer, which shares this model.
        let script: Vec<Vec<MockStreamEvent>> = (0..self.stream_turns)
            .map(|_| {
                vec![
                    MockStreamEvent::Text("mock response".to_string()),
                    MockStreamEvent::final_response_with_default_usage(),
                ]
            })
            .collect();
        let model = MockCompletionModel::from_stream_turns(script);
        let compactor_model = model.clone();
        let config = Config {
            provider: "ollama".to_string(),
            // The session's block serves the ollama-cloud cache section, as a
            // real `[providers.ollama] catalog = "ollama-cloud"` block does.
            catalog: Some("ollama-cloud".to_string()),
            model: "llama3".to_string(),
            // The composed runtime derives both the compaction threshold and the
            // compactor's planning spec from this window. A small window lets a
            // short conversation outgrow the policy's keep window.
            model_context_tokens: Some(MOCK_CONTEXT_TOKENS),
            max_output_tokens: Some(1_024),
            ..Config::default()
        };
        let provider =
            ProviderState::with_client(config.clone(), CachedProviderClient::Mock(model));
        let engine: Arc<dyn agnt_runtime::tools::closure::EngineInterfaceLike> =
            Arc::new(crate::AcpEngineStub::new(cwd.to_string_lossy().to_string()));
        // The composed runtime derives the compaction threshold from the active
        // config, so the builder states its token count as the fraction of that
        // window the count is.
        let proactive_threshold_pct = self
            .threshold_tokens
            .map(|tokens| tokens as f64 / MOCK_CONTEXT_TOKENS as f64);

        let runtime = AgentConversationRuntime::compose(
            RuntimeParts {
                provider,
                tools: ToolState::new(Vec::new(), Vec::new(), ClosureRegistry::default()),
                session: MemoryState::new(rig.clone()),
                persona: PersonaState::new(None, None, None, None, None, None),
                multi: MultiAgentState::new(Vec::new(), AgentsConfig::default()),
                engine: Arc::clone(&engine),
                bus: bus.clone(),
            },
            RuntimeBuildParams {
                runtime: runtime_handle,
                config,
                plugin_config: None,
                tool_definitions: Vec::new(),
                baseline_tool_definitions: Vec::new(),
                closure_registry: ClosureRegistry::default(),
                mcp_runtime: None,
                tool_server_handle: agnt_core::types::ToolServer::new().run(),
                mcp_lifecycle_projection: Vec::new(),
                mcp_server_configs: Vec::new(),
                mcp_caller_cwd: Some(cwd.clone()),
                vault: None,
                mcp_registry: McpToolRegistry::empty(),
                engine,
                rig,
                final_session_id: session_id.clone(),
                proactive_threshold_pct,
                base_permissions: PermissionsConfig::safe_defaults(true),
                effective_permissions: PermissionsConfig::safe_defaults(true),
                cli_permissions_overlay: None,
                permissions_startup_summary: String::new(),
                persona_body: None,
                agent_identity: None,
                agent_description: None,
                agent_icon: None,
                cached_agents_chain: None,
                cached_available_skills: None,
                available_agents: Vec::new(),
                agents_config: AgentsConfig::default(),
                cwd,
                paths,
                bus,
                hook_context,
                compactor_model: Some(Arc::new(Mutex::new(compactor_model.erase()))),
            },
        )
        .await
        .map_err(|e| AcpError::Runtime(format!("failed to build mock runtime: {e}")))?;

        // A test can seed this session's cumulative usage, so two sessions can
        // carry distinct totals. The handle reads the same cells the runtime
        // accumulates into.
        if let Some((id, input, output)) = &self.usage_for_session
            && session_id.as_deref() == Some(id.as_str())
        {
            runtime.session_usage_handle().add_call(
                *input,
                *output,
                Some(input + output),
                None,
                false,
            );
        }

        Ok(runtime.with_decision_source(Some(Arc::new(Mutex::new(HashMap::new())))))
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

/// The rig-memory store config the manager reads conversations from.
///
/// File-backed under `dir`, so every `open` reaches the same database.
pub(super) fn rig_config(dir: &Path) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(dir)
}

/// The application paths of a session test rooted at `dir`.
///
/// The config file and the persona dir sit under `dir`, so the manager never
/// reads the user's real config or persona files.
pub(super) fn test_paths(dir: &Path) -> agnt_runtime::config::Paths {
    let logs = dir.join("logs");
    agnt_runtime::config::Paths {
        config_path: dir.join("config.toml"),
        dotenv_path: dir.join(".env"),
        agents_dir: dir.join("agents"),
        copilot_token_dir: dir.join("github_copilot"),
        store_path: dir.join("sessions.db"),
        log_dir: logs.clone(),
        log_path: logs.join("agent.log"),
        audit_log_path: dir.join("tool_audit.log"),
        secrets_path: dir.join("secrets.json"),
        models_cache_path: dir.join("models.json"),
        theme_path: dir.join("theme.json"),
        grammar_cache_dir: dir.join("tree-sitter"),
    }
}

pub(super) async fn manager(dir: &Path) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    ))
}

/// A manager whose runtime builder is `builder`, so a test can configure the
/// mock model's script and the chain's compaction threshold.
pub(super) async fn manager_with_builder(
    dir: &Path,
    builder: MockRuntimeBuilder,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        builder,
    ))
}

pub(super) fn session_cwd(dir: &Path, name: &str) -> Result<PathBuf> {
    let cwd = dir.join(name);
    std::fs::create_dir_all(&cwd)?;
    Ok(cwd)
}

/// A manager whose runtime builder uses `bus` for every session it builds.
///
/// Lets a test subscribe to the session channel before a session is opened, so
/// the `SessionEvent` the manager publishes is not missed.
pub(super) async fn manager_with_bus(
    dir: &Path,
    bus: Bus,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        },
    ))
}

/// A manager whose runtime builder uses `bus`, plus the rig-memory store config
/// it reads conversations from.
pub(super) async fn manager_with_bus_and_rig_store(
    dir: &Path,
    bus: Bus,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        },
    );
    Ok((manager, rig))
}

/// A manager whose builder seeds `input`/`output` tokens into the runtime of
/// `session_id`, so two sessions can carry distinct cumulative totals.
pub(super) async fn manager_with_usage_for_session(
    dir: &Path,
    bus: Bus,
    session_id: &str,
    input: u64,
    output: u64,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder {
            bus: Some(bus),
            ..MockRuntimeBuilder::default()
        }
        .with_usage_for_session(session_id, input, output),
    );
    Ok((manager, rig))
}

/// A manager plus the rig-memory store config it reads conversations from, so a
/// test can seed a persisted session.
pub(super) async fn manager_with_rig_store(
    dir: &Path,
) -> Result<(AcpSessionManager<MockRuntimeBuilder>, RigStoreConfig)> {
    let rig = rig_config(dir);
    let manager = AcpSessionManager::with_runtime_builder(
        rig.clone(),
        test_paths(dir),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    );
    Ok((manager, rig))
}

/// Persist a two-message conversation under `session_id` in the rig store.
pub(super) async fn seed_session(rig: &RigStoreConfig, session_id: &str) -> Result<()> {
    let store = rig
        .open(session_id)
        .await
        .map_err(|e| format!("should open rig store: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("should load rig conversation: {e:?}"))?;
    conversation.extend([
        Message::user("hello".to_string()),
        Message::assistant("hi there".to_string()),
    ]);
    rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("should seed rig conversation: {e:?}"))?;
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
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
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
    let captured = Arc::new(Mutex::new(Vec::new()));
    let manager = AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        test_paths(dir),
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

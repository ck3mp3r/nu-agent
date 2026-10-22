use std::sync::Arc;

use agnt_testkit::MockServer;
use nu_protocol::LabeledError;
use rig::test_utils::MockCompletionModel;

use crate::conversation::providers::CachedProviderClient;
use crate::conversation::state::memory::MemoryState;
use crate::conversation::turn::executor::test_utils::{
    BusEventCollector, MockResolver, test_config,
};
use crate::conversation::turn::executor::*;
use crate::conversation::turn::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::types::Message;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn default_tool_infra(
    handle: rig::tool::server::ToolServerHandle,
    definitions: Vec<rig::completion::ToolDefinition>,
) -> ToolInfra {
    ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: definitions,
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
        render_registry: ToolRenderRegistry::default(),
    }
}

// ---------------------------------------------------------------------------
// JourneyHarness
// ---------------------------------------------------------------------------

pub(crate) struct JourneyHarness {
    _temp_dir: tempfile::TempDir, // leading underscore keeps TempDir alive
    pub(crate) memory_state: MemoryState,
    dyn_model: std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>>,
    compaction_config: crate::conversation::compaction::CompactionConfig,
    pub(crate) session_id: &'static str,
    config: crate::config::Config,
}

impl JourneyHarness {
    pub(crate) fn new(session_id: &'static str) -> Self {
        Self::new_with_config(session_id, test_config())
    }

    /// Create a harness with a custom config (e.g., to set max_tool_result_bytes).
    pub(crate) fn new_with_config(session_id: &'static str, config: crate::config::Config) -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let bus = crate::bus::create_bus();
        let compaction_config = crate::conversation::compaction::CompactionConfig {
            compactor: crate::conversation::compaction::compactor::NuCompactor::from_shared_model(
                test_utils::shared_mock_model(),
                bus.clone(),
                rig_memory::CompactionPolicy::default(),
                crate::conversation::compaction::compactor::resolve_model_spec(
                    "openai",
                    "gpt-4o",
                    Some(8_000),
                    Some(1_024),
                ),
            ),
            params: crate::compaction::CompactionParams::default(),
            threshold_tokens: None,
        };
        let memory_state = MemoryState::new(crate::session::RigStoreConfig::in_memory());
        Self {
            _temp_dir: temp_dir,
            memory_state,
            dyn_model: test_utils::shared_mock_model(),
            compaction_config,
            session_id,
            config,
        }
    }

    /// Execute one turn. Returns the outcome plus the bus events published during
    /// the turn (as `UiEvent`s converted at the boundary).
    pub(crate) async fn turn(
        &mut self,
        prompt: &str,
        model: MockCompletionModel,
        tool_infra: ToolInfra,
    ) -> (
        std::result::Result<TurnOutcome, LabeledError>,
        Vec<crate::protocol::event::UiEvent>,
    ) {
        // Swap the scripted mock model into the shared slot so the agent
        // (built from the model) routes to this model.
        *self.dyn_model.lock().expect("model mutex poisoned") = model.erase();
        let mut executor = TurnExecutor::new(
            &self.config,
            &mut self.memory_state,
            tool_infra.clone(),
            Arc::clone(&self.dyn_model),
            self.compaction_config.clone(),
        );
        let mut event_collector = BusEventCollector::subscribe(&tool_infra.bus);
        let outcome = executor
            .execute(
                ExecuteInput {
                    prompt: prompt.to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(self.session_id),
            )
            .await;
        let events = event_collector.drain();
        (outcome, events)
    }

    /// Raw messages from the rig-memory store — no repair, no filtering.
    ///
    /// The session store is frozen after S2b, so the conversation the agent
    /// wrote lives in the rig store.
    pub(crate) async fn raw_messages(&self) -> Result<Vec<Message>> {
        let store = self
            .memory_state
            .inner_memory()
            .rig_store(self.session_id)
            .map_err(|e| format!("rig store open: {e:?}"))?;
        let conversation = rig_memory::load(&*store, self.session_id)
            .map_err(|e| format!("rig store load: {e:?}"))?;
        Ok(conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect())
    }

    /// Seed the rig-memory store with `messages` before the first turn.
    pub(crate) fn seed_history(&self, messages: Vec<Message>) -> Result<()> {
        let store = self
            .memory_state
            .inner_memory()
            .rig_store(self.session_id)
            .map_err(|e| format!("rig store open: {e:?}"))?;
        let mut conversation = rig_memory::load(&*store, self.session_id)
            .map_err(|e| format!("rig store load: {e:?}"))?;
        conversation.extend(messages);
        rig_memory::save(&*store, self.session_id, &mut conversation)
            .map_err(|e| format!("rig store save: {e:?}"))?;
        Ok(())
    }

    /// Starts a mock server on the harness runtime.
    /// Caller must keep the returned MockServer alive for the duration of the test —
    /// dropping it early causes connection refused mid-stream.
    pub(crate) async fn start_mock_server(&self) -> Result<(MockServer, CachedProviderClient)> {
        // Start the mock server on a dedicated runtime to avoid conflicts with the harness rt.
        let server = agnt_testkit::start_server().await;
        // Use OpenAiCompletions (which targets /chat/completions) for the mock tests.
        // This is the OpenAI-compatible completions API path, matching our mock setup.
        let http_client = crate::conversation::providers::build_http_client(None)
            .map_err(|e| format!("build test http client: {e}"))?;
        let openai_client = rig::providers::openai::OpenAIConfig::new("fake-key")
            .with_base_url(server.uri())
            .with_route(rig::providers::openai::Route::Chat)
            .connect(rig::http_client::ReqwestClient::from(http_client));
        let cached = CachedProviderClient::OpenAiCompletions(openai_client);
        Ok((server, cached))
    }

    pub(crate) async fn turn_with_client(
        &mut self,
        prompt: &str,
        client: &CachedProviderClient,
        tool_infra: ToolInfra,
    ) -> Result<(
        std::result::Result<TurnOutcome, LabeledError>,
        Vec<crate::protocol::event::UiEvent>,
    )> {
        // Swap the mock client's model into the shared slot so the agent
        // (built from the model) routes to this client's model.
        *self.dyn_model.lock().expect("model mutex poisoned") = client
            .build_dyn_model(&self.config.model, &self.config.provider, None, None)
            .map_err(|e| format!("build model from cached client: {e:?}"))?;
        let mut executor = TurnExecutor::new(
            &self.config,
            &mut self.memory_state,
            tool_infra.clone(),
            Arc::clone(&self.dyn_model),
            self.compaction_config.clone(),
        );
        let mut event_collector = BusEventCollector::subscribe(&tool_infra.bus);
        let outcome = executor
            .execute(
                ExecuteInput {
                    prompt: prompt.to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(self.session_id),
            )
            .await;
        let events = event_collector.drain();
        Ok((outcome, events))
    }
}

// ---------------------------------------------------------------------------
// Tool infrastructure helpers
// ---------------------------------------------------------------------------

/// No tools — for pure text turns.
pub(crate) fn no_tools() -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new().run();
    default_tool_infra(handle, vec![])
}

/// Build a `MemoryState` over a specific session-store path.
///
/// The rig store is file-backed under the same path, so a second
/// `MemoryState` over that path reads the conversation the first one saved.
pub(crate) fn memory_state_at(path: std::path::PathBuf) -> MemoryState {
    MemoryState::new(
        crate::session::RigStoreConfig::new(crate::session::StoreType::Sqlite).with_cache_dir(path),
    )
}

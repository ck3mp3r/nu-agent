//! Shared fixtures for the ACP session manager test modules.
//!
//! Holds the imports and the helpers every topical session test file needs.
//! Sub-files pull them in with `use super::*;`.

// region:    --- Modules

#[path = "support/accessors.rs"]
mod accessors;

#[path = "support/managers.rs"]
mod managers;

#[path = "support/sinks.rs"]
mod sinks;

pub(super) use managers::*;
pub(super) use sinks::*;

// endregion: --- Modules

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
pub(super) use agnt_kernel::{CompactionRecord, SessionRecord};
pub(super) use agnt_runtime::bus::{Bus, CancelEvent, LifecycleEvent, LifecycleRx, OneshotTx};
pub(super) use agnt_runtime::config::{AgentsConfig, Config, ModelsCache};
pub(super) use agnt_runtime::conversation::contracts::DecisionSource;
pub(super) use agnt_runtime::conversation::providers::CachedProviderClient;
pub(super) use agnt_runtime::conversation::runtime_build::RuntimeBuildParams;
pub(super) use agnt_runtime::conversation::runtime_parts::RuntimeParts;
pub(super) use agnt_runtime::conversation::state::memory::MemoryState;
pub(super) use agnt_runtime::conversation::state::{
    MultiAgentState, PersonaState, ProviderState, ToolState,
};
pub(super) use agnt_runtime::protocol::event::PermissionDecision;
pub(super) use agnt_runtime::protocol::events::CompactionSource;
pub(super) use agnt_runtime::session::{RigStoreConfig, StoreType};
pub(super) use agnt_runtime::tools::authz::PermissionsConfig;
pub(super) use agnt_runtime::tools::closure::ClosureRegistry;
pub(super) use agnt_runtime::tools::handler::McpToolRegistry;
pub(super) use agnt_runtime::types::Message;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::*;
use tokio::sync::Mutex as AsyncMutex;

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
            captured
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(client_fs);
        }
        if let Some(captured) = &self.captured_agent_name {
            captured
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(agent_name);
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
        // The wire answers as a model whose input limit is the fixture's window:
        // the chain's trigger judges the request against the model it is sent to,
        // and a model the catalog does not list has no limit to cross.
        let scripted = MockCompletionModel::from_stream_turns(script);
        let model = match agnt_runtime::conversation::compaction::compactor::resolve_model_spec(
            "ollama",
            "llama3",
            Some(MOCK_CONTEXT_TOKENS as u32),
            Some(1_024),
            None,
        ) {
            Some(spec) => scripted.priced_as(spec),
            None => scripted,
        };
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
        // The composed runtime gates on rig's `reserve_tokens` and the turn's own
        // spec, so the builder states how many tokens of that window the decision
        // must leave free for the count it wants to trip it.
        let reserve_tokens = self
            .threshold_tokens
            .map(|tokens| MOCK_CONTEXT_TOKENS.saturating_sub(tokens) as u64);
        let plugin_config = agnt_runtime::config::PluginConfig {
            compaction: Some(agnt_runtime::config::CompactionSettings {
                reserve_tokens: reserve_tokens.unwrap_or_default(),
                enabled: reserve_tokens.is_some(),
                // Keeps only the newest message — the turn's prompt — so the
                // side-call summarizes rather than finding nothing to do.
                keep_recent_tokens: 1,
            }),
            ..Default::default()
        };

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
                plugin_config: Some(plugin_config),
                tool_definitions: Vec::new(),
                baseline_tool_definitions: Vec::new(),
                closure_registry: ClosureRegistry::default(),
                mcp_runtime: None,
                tool_server_handle: agnt_runtime::types::ToolServer::new().run(),
                mcp_lifecycle_projection: Vec::new(),
                mcp_server_configs: Vec::new(),
                mcp_caller_cwd: Some(cwd.clone()),
                vault: None,
                mcp_registry: McpToolRegistry::empty(),
                engine,
                rig,
                final_session_id: session_id.clone(),
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

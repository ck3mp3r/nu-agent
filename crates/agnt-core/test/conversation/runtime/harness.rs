//! Shared harness for runtime tests that drive the real
//! `AgentConversationRuntime` through its trait methods.
//!
//! The runtime is hand-constructed (every field is `pub`) with a
//! `MockCompletionModel` injected through `ProviderState::with_client`, so no
//! HTTP transport, TLS provider, keychain, or database is touched.

use std::sync::{Arc, Mutex};

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::*;
use crate::bus::Bus;
use crate::config::{AgentsConfig, Config};
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::NuCompactor;
use crate::conversation::providers::CachedProviderClient;
use crate::conversation::state::mcp::McpState;
use crate::conversation::state::memory::MemoryState;
use crate::conversation::state::multi_agent::MultiAgentState;
use crate::conversation::state::permission::PermissionState;
use crate::conversation::state::persona::PersonaState;
use crate::conversation::state::provider::ProviderState;
use crate::conversation::state::tool::ToolState;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::tools::authz::{PermissionsConfig, SessionGrantCache};
use crate::tools::closure::{ClosureRegistry, CwdEngine};
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

/// A constructed runtime plus the tempdir backing its session store.
pub(super) struct RuntimeFixture {
    pub(super) runtime: AgentConversationRuntime,
    pub(super) temp_dir: tempfile::TempDir,
    pub(super) bus: Bus,
}

impl RuntimeFixture {
    /// Root of the fixture's tempdir — the runtime cwd and the store root.
    pub(super) fn root(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Write a persona file into `<cwd>/.agents/<name>.md` so `switch_agent`
    /// resolves it from the cwd directory.
    pub(super) fn write_persona(&self, name: &str, contents: &str) -> Result<()> {
        let dir = self.root().join(".agents");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{name}.md")), contents)?;
        Ok(())
    }
}

/// A runtime config that never reaches a real provider: the mock client is
/// injected, and retries are disabled so a failing turn fails fast.
pub(super) fn runtime_config() -> Config {
    Config {
        provider: "mock".to_string(),
        model: "mock-model".to_string(),
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..Config::default()
    }
}

/// A deterministic streaming model used by the compactor so compaction never
/// invokes a real LLM.
fn summary_model() -> MockCompletionModel {
    let turns: Vec<Vec<MockStreamEvent>> = (0..4)
        .map(|_| {
            vec![
                MockStreamEvent::Text("summary".to_string()),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    MockCompletionModel::from_stream_turns(turns)
}

/// Build a runtime whose shared model and cached provider client are both the
/// given mock, with the MCP caller cwd set to the fixture root.
pub(super) fn make_runtime(model: MockCompletionModel) -> Result<RuntimeFixture> {
    build_fixture(model, true, McpToolRegistry::empty(), Vec::new())
}

/// Build a runtime with no MCP caller cwd, so `switch_agent` reports the
/// working directory as unavailable.
pub(super) fn make_runtime_without_caller_cwd(
    model: MockCompletionModel,
) -> Result<RuntimeFixture> {
    build_fixture(model, false, McpToolRegistry::empty(), Vec::new())
}

/// Build a runtime with a populated MCP registry and matching tool
/// definitions, so the MCP visibility counts are non-trivial.
pub(super) fn make_runtime_with_mcp(
    model: MockCompletionModel,
    registry: McpToolRegistry,
    tool_definitions: Vec<ToolDefinition>,
) -> Result<RuntimeFixture> {
    build_fixture(model, true, registry, tool_definitions)
}

fn build_fixture(
    model: MockCompletionModel,
    with_caller_cwd: bool,
    registry: McpToolRegistry,
    tool_definitions: Vec<ToolDefinition>,
) -> Result<RuntimeFixture> {
    let temp_dir = tempfile::tempdir()?;
    let cwd = temp_dir.path().to_path_buf();
    let bus = crate::bus::create_bus();
    let provider =
        ProviderState::with_client(runtime_config(), CachedProviderClient::Mock(model.clone()));
    let compaction = CompactionConfig {
        compactor: NuCompactor::from_shared_model(
            Arc::new(Mutex::new(summary_model().erase())),
            bus.clone(),
            rig_memory::CompactionPolicy::default(),
            crate::conversation::compaction::compactor::resolve_model_spec(
                "openai",
                "gpt-4o",
                Some(200_000),
                Some(1_024),
            ),
        ),
        params: crate::compaction::CompactionParams::default(),
        threshold_tokens: None,
        threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
    };

    let runtime = AgentConversationRuntime {
        runtime: tokio::runtime::Handle::current(),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        mcp_state: McpState::new(
            None,
            Vec::new(),
            Vec::new(),
            with_caller_cwd.then(|| cwd.clone()),
            registry,
            4096,
            None,
        ),
        permission_state: PermissionState::new(
            PermissionsConfig::safe_defaults(true),
            PermissionsConfig::safe_defaults(true),
            None,
            SessionGrantCache::default(),
            "test permissions summary".to_string(),
        ),
        engine: CwdEngine::shared(cwd.to_string_lossy().to_string()),
        final_session_id: Some("test-session".to_string()),
        cwd,
        paths: crate::test_support::paths_in(temp_dir.path()),
        interactive_pending: None,
        renders_tool_previews: false,
        circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
        doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
        output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
        repetition_guard: true,
        last_total_tokens: Arc::new(Mutex::new(None)),
        session_usage: crate::hook::TurnUsage::default(),
        bus: bus.clone(),
        render_registry: ToolRenderRegistry::default(),
        provider,
        tools: ToolState::new(
            tool_definitions.clone(),
            tool_definitions,
            ClosureRegistry::default(),
        ),
        session: MemoryState::new(crate::session::RigStoreConfig::in_memory()),
        persona: PersonaState::new(
            Some("persona body".to_string()),
            Some("test-agent".to_string()),
            Some("test description".to_string()),
            Some("robot".to_string()),
            Some("agents chain".to_string()),
            Some("available skills".to_string()),
        ),
        multi_agent: MultiAgentState::new(Vec::new(), AgentsConfig::default()),
        dyn_model: Arc::new(Mutex::new(model.erase())),
        compaction,
        hook_context: crate::hooks::HookAgentContext::default(),
    };

    Ok(RuntimeFixture {
        runtime,
        temp_dir,
        bus,
    })
}

/// A single-turn mock that streams `text` and finishes with default usage.
pub(super) fn text_model(text: &str) -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([vec![
        MockStreamEvent::Text(text.to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]])
}

use std::sync::Arc;

use rig::test_utils::MockCompletionModel;

use super::super::*;
use crate::config::Config;
use crate::conversation::state::memory::MemoryOf;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::session::FsSessionStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

pub(crate) fn default_circuit_breaker() -> Arc<std::sync::Mutex<McpCircuitBreaker>> {
    Arc::new(std::sync::Mutex::new(McpCircuitBreaker::default()))
}

pub(crate) fn default_doom_state() -> Arc<std::sync::Mutex<DoomLoopState>> {
    Arc::new(std::sync::Mutex::new(DoomLoopState::default()))
}

pub(crate) fn default_output_repetition() -> Arc<std::sync::Mutex<RepetitionState>> {
    Arc::new(std::sync::Mutex::new(RepetitionState::default()))
}

/// Repetition guard enabled by default so existing tests exercise detection.
pub(crate) fn default_repetition_guard() -> bool {
    true
}

/// A `last_total_tokens` slot starting at `None` (no real token count yet).
pub(crate) fn default_last_total_tokens() -> Arc<std::sync::Mutex<Option<u64>>> {
    Arc::new(std::sync::Mutex::new(None))
}

/// Build a `MemoryOf<FsSessionStore>` over a tempdir store (no compaction —
/// `CachedMemory` is used directly).
pub(super) fn make_compacting_memory() -> (tempfile::TempDir, MemoryOf<FsSessionStore>) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = Arc::new(crate::session::CachedMemory::new(store));
    (temp_dir, memory)
}

/// Build a `CompactionConfig<FsSessionStore>` (no marker store) with a
/// deterministic streaming mock model so compaction never invokes a real LLM.
pub(super) fn test_compaction_config(
    bus: crate::bus::Bus,
) -> crate::conversation::compaction::CompactionConfig<FsSessionStore> {
    use crate::conversation::compaction::compactor::NuCompactor;
    use rig::test_utils::MockCompletionModel;

    let turns: Vec<Vec<rig::test_utils::MockStreamEvent>> = (0..8)
        .map(|_| {
            vec![
                rig::test_utils::MockStreamEvent::Text("summary".to_string()),
                rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    crate::conversation::compaction::CompactionConfig {
        compactor: NuCompactor::from_shared_model(
            std::sync::Arc::new(std::sync::Mutex::new(model.erase())),
            bus.clone(),
            None,
        ),
        params: crate::compaction::CompactionParams::default(),
        threshold_tokens: None,
    }
}

// ---------------------------------------------------------------------------
// MockResolver: AsyncPermissionResolver that always returns Allow
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(super) struct MockResolver(pub(super) PermissionDecision);

impl AsyncPermissionResolver for MockResolver {
    fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &crate::bus::Bus,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = self.0.clone();
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(super) fn make_turn_context<'a>(
    dyn_model: std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>>,
    config: &'a Config,
    bus: crate::bus::Bus,
) -> TurnContext<'a, FsSessionStore> {
    let (temp_dir, memory) = make_compacting_memory();
    let _keep_alive = temp_dir;
    let conversation = TurnConversation {
        memory,
        conversation_id: "test-conv".to_string(),
        has_session: true,
        dyn_model,
        compaction: test_compaction_config(bus.clone()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus,
        render_registry: ToolRenderRegistry::default(),
    };
    TurnContext::new(conversation, input, tool_infra, config)
}

/// Wrap a `MockCompletionModel` in the shared `Arc<Mutex<DynModel<Completion>>>`
/// so the agent (built from the model) routes to the scripted model.
pub(super) fn shared_handle(
    model: MockCompletionModel,
) -> std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>> {
    std::sync::Arc::new(std::sync::Mutex::new(model.erase()))
}

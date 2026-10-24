//! Shared fixtures for the hook chain test modules.
//!
//! Holds the imports, the mock session store, the compaction and repetition
//! fixtures, and the preview stubs every topical chain test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use std::sync::{Arc, Mutex};

pub(super) use crate::bus::Bus;
pub(super) use crate::compaction::CompactionParams;
pub(super) use crate::conversation::compaction::CompactionConfig;
pub(super) use crate::conversation::compaction::compactor::NuCompactor;
pub(super) use crate::hook::chain::HookEnv;
pub(super) use crate::hook::doom_loop::DoomLoopState;
pub(super) use crate::hook::output_repetition::RepetitionState;
pub(super) use crate::hook::permission_resolver::PolicyPermissionResolver;
pub(super) use crate::hook::{HookChain, HookState};
pub(super) use crate::protocol::event::UiEvent;
pub(super) use crate::session::CachedMemory;

pub(super) use futures::StreamExt;
pub(super) use rig::DynModel;
pub(super) use rig::operation::Completion;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};
pub(super) use tempfile::TempDir;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Compaction threshold with real token count
// ---------------------------------------------------------------------------

/// Shared fixtures for the `decide_compaction` threshold tests: an in-memory
/// store (no markers), a compactor backed by a deterministic mock model, an
/// empty `last_total_tokens` slot, and the compaction config + bus.
pub(super) struct DecideFixture {
    pub(super) memory: Arc<CachedMemory>,
    pub(super) compaction: CompactionConfig,
    pub(super) bus: Bus,
    pub(super) last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// The summarizer model, kept so a test can assert it was never called.
    pub(super) summary_model: MockCompletionModel,
}

/// Build the fixtures shared by the `decide_compaction` threshold tests.
pub(super) fn decide_fixture() -> Result<DecideFixture> {
    let summary_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(
        summary_model.clone().erase(),
        Bus::default(),
        rig_memory::CompactionPolicy::default(),
        test_model_spec(),
    );
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let bus = Bus::default();
    let compaction = CompactionConfig {
        compactor,
        params: CompactionParams::default(),
        threshold_tokens: Some(80_000),
        threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
    };
    let last_total_tokens = Arc::new(Mutex::new(None));
    Ok(DecideFixture {
        memory,
        compaction,
        bus,
        last_total_tokens,
        summary_model,
    })
}

/// The model spec the test compactors plan against.
pub(super) fn test_model_spec() -> Option<rig::catalog::ModelSpec> {
    crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(200_000),
        Some(1_024),
    )
}

// ---------------------------------------------------------------------------
// Hook chain construction
// ---------------------------------------------------------------------------

/// Build a hook chain over `bus` with `permission` as its resolver, no tools
/// registered, and no scripted model. The chain subscribes to the cancel
/// channel in `new`, so build it before publishing any cancel.
pub(super) fn build_hook_with<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    bus: Bus,
    permission: P,
) -> HookChain<P> {
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    HookChain::new(
        HookEnv {
            bus,
            cwd: std::path::PathBuf::from("/tmp"),
        },
        permission,
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        crate::tools::handler::builtin_tool::ToolRenderRegistry::default(),
        None,
        HookState {
            circuit_breaker: Arc::new(Mutex::new(
                crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
            )),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: true,
            memory,
            conversation_id: "chain-test-conv".to_string(),
            compaction: CompactionConfig {
                compactor: NuCompactor::new(
                    MockCompletionModel::text("summary").erase(),
                    Bus::default(),
                    rig_memory::CompactionPolicy::default(),
                    test_model_spec(),
                ),
                params: CompactionParams::default(),
                threshold_tokens: None,
                threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
            },
            model: MockCompletionModel::default().erase(),
            last_total_tokens: Arc::new(Mutex::new(None)),
        },
    )
}

// ---------------------------------------------------------------------------
// Output-repetition hook behavior: warn-only intra-stream, cross-turn ladder
// ---------------------------------------------------------------------------

/// Test harness running the real `HookChain` through real rig streaming
/// agents. Rig's stream ends after ONE model turn, so cross-turn scenarios
/// build a FRESH agent per `turn()` call while the `HookChain` state Arcs
/// are shared across them — exactly how the executor runs production turns.
/// Each turn builds its agent from the queued model.
pub(super) struct RepetitionTurnFixture {
    pub(super) bus: Bus,
    dyn_model: Arc<std::sync::Mutex<DynModel<Completion>>>,
    hook_state: HookState,
    _temp_dir: TempDir, // keeps the store backing the memory alive
}

impl RepetitionTurnFixture {
    /// Build the fixture: shared hook state over a tempdir-backed memory, a
    /// queued scripted model, and a real bus.
    pub(super) fn new() -> Result<Self> {
        let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
        let memory = Arc::new(CachedMemory::new(
            crate::session::RigStoreConfig::in_memory(),
        ));
        let dyn_model = Arc::new(std::sync::Mutex::new(
            MockCompletionModel::default().erase(),
        ));
        Ok(Self {
            bus: Bus::default(),
            dyn_model: dyn_model.clone(),
            hook_state: HookState {
                circuit_breaker: Arc::new(std::sync::Mutex::new(
                    crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
                )),
                doom_state: Arc::new(std::sync::Mutex::new(DoomLoopState::default())),
                output_repetition: Arc::new(std::sync::Mutex::new(RepetitionState::default())),
                repetition_guard: true,
                memory,
                conversation_id: "chain-test-conv".to_string(),
                compaction: CompactionConfig {
                    compactor: NuCompactor::new(
                        MockCompletionModel::text("summary").erase(),
                        Bus::default(),
                        rig_memory::CompactionPolicy::default(),
                        test_model_spec(),
                    ),
                    params: CompactionParams::default(),
                    threshold_tokens: None,
                    threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
                },
                model: dyn_model.lock().expect("model mutex poisoned").clone(),
                last_total_tokens: Arc::new(std::sync::Mutex::new(None)),
            },
            _temp_dir: temp_dir,
        })
    }

    /// Queue a script of one or more streaming turns on the shared model.
    /// A tool-call turn needs TWO model calls in the same script:
    /// the tool call, then the post-tool-result final text.
    pub(super) fn queue_turn(&self, turns: Vec<Vec<MockStreamEvent>>) {
        *self.dyn_model.lock().expect("model mutex poisoned") =
            MockCompletionModel::from_stream_turns(turns).erase();
    }

    /// Run one turn: build a fresh agent carrying this fixture's hook chain
    /// (fresh chain per call, but state Arcs are shared), drive the rig
    /// stream to completion, and return the stream items. `with_tool` adds a
    /// static tool to the agent (for tool-call turn scripts).
    pub(super) async fn turn(
        &self,
        with_tool: bool,
    ) -> core::result::Result<
        Vec<core::result::Result<rig::agent::MultiTurnStreamItem, rig::completion::PromptError>>,
        Box<dyn std::error::Error>,
    > {
        let model = self.dyn_model.lock().expect("model mutex poisoned").clone();
        let hook = HookChain::new(
            HookEnv {
                bus: self.bus.clone(),
                cwd: std::path::PathBuf::from("/tmp"),
            },
            PolicyPermissionResolver {
                permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
                session_grants: Arc::new(std::sync::Mutex::new(
                    crate::tools::authz::SessionGrantCache::default(),
                )),
                closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
                mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
            },
            Arc::new(crate::tools::closure::ClosureRegistry::default()),
            Arc::new(crate::tools::handler::McpToolRegistry::empty()),
            crate::tools::handler::builtin_tool::ToolRenderRegistry::default(),
            None,
            HookState {
                circuit_breaker: self.hook_state.circuit_breaker.clone(),
                doom_state: self.hook_state.doom_state.clone(),
                output_repetition: self.hook_state.output_repetition.clone(),
                repetition_guard: self.hook_state.repetition_guard,
                memory: self.hook_state.memory.clone(),
                conversation_id: self.hook_state.conversation_id.clone(),
                compaction: self.hook_state.compaction.clone(),
                model: model.clone(),
                last_total_tokens: self.hook_state.last_total_tokens.clone(),
            },
        );
        let builder = rig::agent::AgentBuilder::new(model).add_hook(hook);
        let agent = if with_tool {
            builder.tool(rig::test_utils::MockAddTool).build()
        } else {
            builder.build()
        };
        let mut stream = agent.prompt("run").max_turns(16).stream();
        let mut items = Vec::new();
        while let Some(item) = stream.next().await {
            items.push(item);
        }
        Ok(items)
    }
}

/// Drain warning messages from an EXISTING subscriber (must be subscribed
/// before the events are published — broadcast sends are not buffered for
/// later subscribers).
pub(super) fn warnings(rx: &mut crate::bus::UiEventRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let UiEvent::Warning { message } = event {
            out.push(message);
        }
    }
    out
}

/// A script of `count` identical text deltas followed by a final response.
pub(super) fn identical_text_turn(count: usize) -> Vec<MockStreamEvent> {
    let mut turn = vec![MockStreamEvent::Text("I will do the thing. ".to_string()); count];
    turn.push(MockStreamEvent::final_response_with_default_usage());
    turn
}

// ---------------------------------------------------------------------------
// suppress_previewed_display: the race-free gate between a pre-authorize
// preview shown before the tool ran and the completion event's own display.
// ---------------------------------------------------------------------------

/// Stub resolver whose `take_previewed` answer is fixed at construction, so
/// `suppress_previewed_display` can be tested as a pure function without
/// rig's opaque `OutcomeEvent` type or a real permission flow.
#[derive(Clone)]
pub(super) struct StubPreviewResolver {
    pub(super) previewed: bool,
}

impl crate::hook::permission_resolver::AsyncPermissionResolver for StubPreviewResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &Bus,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        crate::hook::permission_resolver::PermissionDecision::Allow
    }

    fn take_previewed(&self, _tool_name: &str, _arguments: &str) -> bool {
        self.previewed
    }
}

pub(super) fn sample_display() -> crate::protocol::event::ToolDisplay {
    crate::protocol::event::ToolDisplay {
        title: Some("edit foo.rs".to_string()),
        sections: vec![],
    }
}

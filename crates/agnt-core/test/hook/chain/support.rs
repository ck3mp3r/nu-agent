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
pub(super) use crate::hook::doom_loop::DoomLoopState;
pub(super) use crate::hook::output_repetition::RepetitionState;
pub(super) use crate::hook::permission_resolver::PolicyPermissionResolver;
pub(super) use crate::hook::{HookChain, HookState};
pub(super) use crate::protocol::event::UiEvent;
pub(super) use crate::session::{CachedMemory, FsSessionStore, SessionStore, StoreEntry};
pub(super) use crate::types::Message;

pub(super) use futures::StreamExt;
pub(super) use rig::DynModel;
pub(super) use rig::operation::Completion;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};
pub(super) use tempfile::TempDir;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A `SessionStore` whose `load` always fails, used to verify store errors are
/// surfaced (not swallowed) by `load_marker_context`.
#[derive(Clone)]
pub(super) struct FailingStore;

#[derive(Debug)]
pub(super) struct StoreError;

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store exploded")
    }
}

impl std::error::Error for StoreError {}

impl SessionStore for FailingStore {
    type Error = StoreError;

    async fn create(
        &self,
        _id: &str,
        _first_messages: &[Message],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn load(
        &self,
        _id: &str,
    ) -> core::result::Result<Option<(crate::session::SessionMetadata, Vec<StoreEntry>)>, Self::Error>
    {
        Err(StoreError)
    }

    async fn append(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn replace_entries(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn list(&self) -> core::result::Result<Vec<crate::session::SessionInfo>, Self::Error> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> core::result::Result<(), Self::Error> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Compaction threshold with real token count
// ---------------------------------------------------------------------------

/// Shared fixtures for the `decide_compaction` threshold tests: an in-memory
/// store (no markers), a compactor backed by a deterministic mock model, an
/// empty `last_total_tokens` slot, and the compaction config + bus.
pub(super) struct DecideFixture {
    pub(super) memory: Arc<CachedMemory<FsSessionStore>>,
    pub(super) compaction: CompactionConfig<FsSessionStore>,
    pub(super) bus: Bus,
    pub(super) last_total_tokens: Arc<Mutex<Option<u64>>>,
}

/// Build the fixtures shared by the `decide_compaction` threshold tests.
pub(super) fn decide_fixture() -> Result<DecideFixture> {
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(model.erase(), Bus::default(), None).with_store(store.clone());
    let memory = Arc::new(CachedMemory::new(store));
    let bus = Bus::default();
    let compaction = CompactionConfig {
        compactor,
        params: CompactionParams::default(),
        threshold_tokens: Some(80_000),
    };
    let last_total_tokens = Arc::new(Mutex::new(None));
    Ok(DecideFixture {
        memory,
        compaction,
        bus,
        last_total_tokens,
    })
}

/// A short single-message history (so the per-turn context is not empty) plus a
/// short prompt. The context chars/4 estimate is far below the threshold; the
/// decision must be driven by `last_total_tokens` when it is `Some`.
pub(super) fn small_history_and_prompt() -> (Vec<Message>, Message) {
    (
        vec![Message::user("short context message")],
        Message::user("hi"),
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
    hook_state: HookState<FsSessionStore>,
    _temp_dir: TempDir, // keeps the store backing the memory alive
}

impl RepetitionTurnFixture {
    /// Build the fixture: shared hook state over a tempdir-backed memory, a
    /// queued scripted model, and a real bus.
    pub(super) fn new() -> Result<Self> {
        let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
        let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
        let memory = Arc::new(CachedMemory::new(store));
        Ok(Self {
            bus: Bus::default(),
            dyn_model: Arc::new(std::sync::Mutex::new(
                MockCompletionModel::default().erase(),
            )),
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
                        None,
                    ),
                    params: CompactionParams::default(),
                    threshold_tokens: None,
                },
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
        let hook = HookChain::new(
            self.bus.clone(),
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
                last_total_tokens: self.hook_state.last_total_tokens.clone(),
            },
        );
        let model = self.dyn_model.lock().expect("model mutex poisoned").clone();
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

//! Shared fixtures for the hook chain test modules.
//!
//! Holds the imports, the mock session store, the compaction and repetition
//! fixtures, and the preview stubs every topical chain test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use std::sync::{Arc, Mutex};

pub(super) use crate::bus::Bus;
pub(super) use crate::conversation::compaction::compactor::NuCompactor;
pub(super) use crate::hook::chain::HookEnv;
pub(super) use crate::hook::doom_loop::DoomLoopState;
pub(super) use crate::hook::output_repetition::RepetitionState;
pub(super) use crate::hook::permission_resolver::PolicyPermissionResolver;
pub(super) use crate::hook::{HookChain, HookState};
pub(super) use crate::session::CachedMemory;
pub(super) use agnt_kernel::NoticeKind;
pub(super) use agnt_kernel::bus::NarrationEvent;

pub(super) use futures::StreamExt;
pub(super) use rig::DynModel;
pub(super) use rig::operation::Completion;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};
pub(super) use tempfile::TempDir;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Compaction threshold with real token count
// ---------------------------------------------------------------------------

/// Shared fixtures for the `decide_compaction` trigger tests: an in-memory
/// store (no markers), a compactor carrying rig's compaction settings, an
/// empty meter, the bus, and the spec the decision measures against (a
/// 10k-token input limit, so a short conversation is under it).
pub(super) struct DecideFixture {
    pub(super) memory: Arc<CachedMemory>,
    pub(super) compaction: NuCompactor,
    pub(super) bus: Bus,
    pub(super) meter: crate::hook::AnchorMeter,
    pub(super) pending: crate::hook::chain::PendingSummary,
    pub(super) spec: rig::catalog::ModelSpec,
    /// The model this turn sends to: every side-call runs on it.
    pub(super) model: DynModel<Completion>,
    /// The same model, kept so a test can assert what the side-call asked it.
    pub(super) turn_model: MockCompletionModel,
    /// The summarizer model, kept so a test can assert it was never called.
    pub(super) summary_model: MockCompletionModel,
}

/// Build the fixtures shared by the `decide_compaction` trigger tests.
pub(super) fn decide_fixture() -> Result<DecideFixture> {
    let summary_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let bus = Bus::default();
    let compactor = NuCompactor::new(
        summary_model.clone().erase(),
        bus.clone(),
        // Keeps only the newest message — this turn's prompt — so a short
        // conversation is still summarized and the cut splits no turn.
        rig_memory::CompactionSettings {
            keep_recent_tokens: 1,
            ..rig_memory::CompactionSettings::default()
        },
        test_model_spec(),
    );
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let compaction = compactor;
    // The turn's model answers the side-calls the decision asks for; every call
    // consumes one scripted streaming turn. It carries the turn's spec, as the
    // wire serving a real turn does.
    let spec = spec_with_input_limit(60_000)?;
    let turn_model = MockCompletionModel::from_stream_turns((0..4).map(|_| {
        [
            MockStreamEvent::Text("summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]
    }))
    .priced_as(spec.clone());
    Ok(DecideFixture {
        memory,
        compaction,
        bus,
        meter: crate::hook::AnchorMeter::default(),
        pending: crate::hook::chain::PendingSummary::default(),
        spec,
        model: turn_model.clone().erase(),
        turn_model,
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
        None,
    )
}

/// A spec whose input limit is `tokens`, so a decision meters against it.
pub(super) fn spec_with_input_limit(tokens: u32) -> Result<rig::catalog::ModelSpec> {
    crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(tokens),
        Some(1_024),
        None,
    )
    .ok_or_else(|| "openai must resolve to a registered provider id".into())
}

// ---------------------------------------------------------------------------
// Hook chain construction
// ---------------------------------------------------------------------------

/// The session state a chain fixture builds: the tree's shared `HookState`
/// plus the two compaction parts a site may repoint. Cloning the fixture keeps
/// the state's Arcs, so consecutive turns accumulate exactly as the executor's
/// do. Set the deltas with the `with_…` setters.
#[derive(Clone)]
pub(super) struct HookStateFixture {
    state: HookState,
    compactor_model: DynModel<Completion>,
    compaction_bus: Bus,
}

impl Default for HookStateFixture {
    fn default() -> Self {
        Self {
            state: HookState {
                circuit_breaker: Arc::new(Mutex::new(
                    crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
                )),
                doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
                output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
                repetition_guard: true,
                memory: Arc::new(CachedMemory::new(
                    crate::session::RigStoreConfig::in_memory(),
                )),
                conversation_id: "chain-test-conv".to_string(),
                compaction: NuCompactor::new(
                    MockCompletionModel::text("summary").erase(),
                    Bus::default(),
                    rig_memory::CompactionSettings::default(),
                    test_model_spec(),
                ),
                model: MockCompletionModel::default().erase(),
                meter: crate::hook::AnchorMeter::default(),
                turn_usage: crate::hook::TurnUsage::default(),
                session_usage: crate::hook::TurnUsage::default(),
                store_latch: Default::default(),
                pending_summary: Default::default(),
            },
            compactor_model: MockCompletionModel::text("summary").erase(),
            compaction_bus: Bus::default(),
        }
    }
}

impl HookStateFixture {
    /// The conversation this hook compacts.
    pub(super) fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.state.conversation_id = conversation_id.into();
        self
    }

    /// Whether the doom-loop and output-repetition detectors run.
    pub(super) fn with_repetition_guard(mut self, on: bool) -> Self {
        self.state.repetition_guard = on;
        self
    }

    /// Seed the doom detector with `count` identical call signatures, so the
    /// next call of `tool` with `args` trips it: the fifth matching signature
    /// is the first detection in the turn attempt.
    pub(super) fn with_doom_signatures(self, tool: &str, args: &str, count: usize) -> Self {
        *self
            .state
            .doom_state
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = DoomLoopState {
            recent_signatures: vec![(tool.to_string(), args.to_string()); count],
            escalation_count: 0,
        };
        self
    }

    /// The escalation counter the doom detector carries into the turn: `4`
    /// means the next detection stops the run instead of steering.
    pub(super) fn with_doom_escalation(self, escalation_count: usize) -> Self {
        self.state
            .doom_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .escalation_count = escalation_count;
        self
    }

    /// The wire serving the turn's completions.
    pub(super) fn with_model(mut self, model: DynModel<Completion>) -> Self {
        self.state.model = model;
        self
    }

    /// The bus the compactor publishes its rows on.
    pub(super) fn with_compaction_bus(mut self, bus: Bus) -> Self {
        self.compaction_bus = bus;
        self.recompacted()
    }

    /// The summarizer wire the compactor runs on.
    pub(super) fn with_compactor_model(mut self, model: DynModel<Completion>) -> Self {
        self.compactor_model = model;
        self.recompacted()
    }

    /// The compaction trigger's meter, when a test reads what it metered.
    pub(super) fn with_meter(mut self, meter: crate::hook::AnchorMeter) -> Self {
        self.state.meter = meter;
        self
    }

    /// The turn's usage accumulator, when a test reads its sums.
    pub(super) fn with_turn_usage(mut self, turn_usage: crate::hook::TurnUsage) -> Self {
        self.state.turn_usage = turn_usage;
        self
    }

    /// The fixture with its compactor back in step with its model and bus.
    fn recompacted(mut self) -> Self {
        self.state.compaction = NuCompactor::new(
            self.compactor_model.clone(),
            self.compaction_bus.clone(),
            rig_memory::CompactionSettings::default(),
            test_model_spec(),
        );
        self
    }
}

/// Build a chain over `bus` with `permission` as its resolver, no tools
/// registered, and no scripted model, carrying the fixture's state. The chain
/// subscribes to the cancel channel in `new`, so build it before publishing any
/// cancel.
pub(super) fn build_chain<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    bus: Bus,
    permission: P,
    fixture: HookStateFixture,
) -> HookChain<P> {
    build_chain_with_gates(
        bus,
        permission,
        fixture,
        None,
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
    )
}

/// Build a chain over `bus` with the gate limits a test needs: the per-sub-turn
/// tool cap (`None` is the production default) and the MCP registry the
/// circuit-breaker arm reads. Every other part comes from the fixture.
pub(super) fn build_chain_with_gates<
    P: crate::hook::permission_resolver::AsyncPermissionResolver,
>(
    bus: Bus,
    permission: P,
    fixture: HookStateFixture,
    max_tool_calls_per_subturn: Option<usize>,
    mcp_registry: Arc<crate::tools::handler::McpToolRegistry>,
) -> HookChain<P> {
    HookChain::new(
        HookEnv {
            bus,
            cwd: std::path::PathBuf::from("/tmp"),
        },
        permission,
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        mcp_registry,
        max_tool_calls_per_subturn,
        fixture.state,
    )
}

/// A chain over `bus` on the default fixture state, its compactor publishing on
/// the same bus the chain publishes on.
pub(super) fn build_hook_with<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    bus: Bus,
    permission: P,
) -> HookChain<P> {
    let fixture = HookStateFixture::default().with_compaction_bus(bus.clone());
    build_chain(bus, permission, fixture)
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
    session: HookStateFixture,
    _temp_dir: TempDir, // keeps the store backing the memory alive
}

impl RepetitionTurnFixture {
    /// Build the fixture: shared hook state over a tempdir-backed memory, a
    /// queued scripted model, and a real bus.
    pub(super) fn new() -> Result<Self> {
        let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
        let dyn_model = Arc::new(std::sync::Mutex::new(
            MockCompletionModel::default().erase(),
        ));
        let bus = Bus::default();
        // The compactor publishes the compaction rows on the session's bus; the
        // fixture must wire the same one the chain publishes on.
        let session = HookStateFixture::default()
            .with_compaction_bus(bus.clone())
            .with_model(dyn_model.lock().unwrap_or_else(|e| e.into_inner()).clone());
        Ok(Self {
            bus,
            dyn_model,
            session,
            _temp_dir: temp_dir,
        })
    }

    /// Queue a script of one or more streaming turns on the shared model.
    /// A tool-call turn needs TWO model calls in the same script:
    /// the tool call, then the post-tool-result final text.
    pub(super) fn queue_turn(&self, turns: Vec<Vec<MockStreamEvent>>) {
        *self.dyn_model.lock().unwrap_or_else(|e| e.into_inner()) =
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
        let model = self
            .dyn_model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        // A fresh chain per turn, carrying the fixture's state Arcs: that is
        // what makes the cross-turn accumulation match the executor's.
        let mut session = self.session.clone();
        session.state.model = model.clone();
        let hook = build_chain(
            self.bus.clone(),
            PolicyPermissionResolver {
                permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
                session_grants: Arc::new(std::sync::Mutex::new(
                    crate::tools::authz::SessionGrantCache::default(),
                )),
                closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
                mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
            },
            session,
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
pub(super) fn warnings(rx: &mut crate::bus::NarrationRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text,
        } = event
        {
            out.push(text);
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

/// Drain the `InferenceChunk` events from an EXISTING subscriber, as
/// `(delta, aggregated)` pairs in publish order. Subscribe before the turn
/// runs: broadcast sends are not buffered for later subscribers.
pub(super) fn inference_chunks(rx: &mut crate::bus::NarrationRx) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let NarrationEvent::InferenceChunk {
            delta, aggregated, ..
        } = event
        {
            out.push((delta, aggregated));
        }
    }
    out
}

//! Cost and token-accounting tests for the hook chain.
//!
//! The suite is split into topical sub-files. Shared imports and the fixtures
//! live here; each sub-file pulls the chain root's shared items with
//! `use super::super::*;` and this file's fixtures with `use super::*;`.

// region:    --- Modules

#[path = "pricing.rs"]
mod pricing;

#[path = "tokens.rs"]
mod tokens;

#[path = "turn_usage.rs"]
mod turn_usage;

// endregion: --- Modules

use super::*;

use agnt_kernel::bus::NarrationEvent;
use rig::catalog::Catalog;
use rig::completion::Usage as ModelUsage;
use rig::test_utils::MockTurn;

/// A usage that reports both counters, so the catalog can price it.
fn usage() -> ModelUsage {
    ModelUsage::new()
        .input_tokens(1_000)
        .output_tokens(500)
        .total_tokens(1_500)
}

/// The catalog's spec for `reference`, so a wire bound to it is priced.
fn catalog_spec(reference: &str) -> Result<std::sync::Arc<rig::catalog::ModelSpec>> {
    Ok(Catalog::builtin()
        .resolve(reference)
        .map_err(|e| format!("{reference} must be in the built-in catalog: {e}"))?
        .shared())
}

/// A scripted streaming model answering as the catalog's `reference`, with
/// `usage` on its final response.
fn catalog_turn(reference: &str, usage: ModelUsage) -> Result<MockCompletionModel> {
    Ok(MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("done".to_string()),
        MockStreamEvent::final_response(usage),
    ]])
    .priced_as(catalog_spec(reference)?))
}

/// A scripted unary model answering as the catalog's `reference`, with `usage`
/// on its response.
fn catalog_call(reference: &str, usage: ModelUsage) -> Result<MockCompletionModel> {
    Ok(
        MockCompletionModel::from_turns([MockTurn::text("done").with_usage(usage)])
            .priced_as(catalog_spec(reference)?),
    )
}

/// A pricing-less summarizer wire: the compactor's own model, which nothing
/// prices. A fallback that priced with it instead of the turn's wire would
/// emit no cost.
fn pricing_less_summarizer() -> MockCompletionModel {
    MockCompletionModel::text("summary")
}

/// A turn's hook chain, the bus its events arrive on, and the token-count
/// slot the hook stores the completion's real count in.
struct TurnHook {
    hook: HookChain<PolicyPermissionResolver>,
    bus: Bus,
    meter: crate::hook::AnchorMeter,
    turn_usage: crate::hook::TurnUsage,
}

/// Build the hook chain a turn runs with. `model` is the wire serving the
/// turn's completions; `compactor_model` is the summarizer's, which the hook
/// must not price the turn's completions with.
fn turn_hook(
    model: &MockCompletionModel,
    compactor_model: MockCompletionModel,
) -> Result<TurnHook> {
    let bus = Bus::default();
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let meter = crate::hook::AnchorMeter::default();
    let turn_usage = crate::hook::TurnUsage::default();
    let compactor = NuCompactor::new(
        compactor_model.erase(),
        Bus::default(),
        rig_memory::CompactionPolicy::default(),
        test_model_spec(),
    );
    let hook = HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        PolicyPermissionResolver {
            permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
            session_grants: Arc::new(Mutex::new(crate::tools::authz::SessionGrantCache::default())),
            closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
            mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        },
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        None,
        HookState {
            circuit_breaker: Arc::new(Mutex::new(
                crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
            )),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: false,
            memory,
            conversation_id: "cost-test-conv".to_string(),
            compaction: CompactionConfig {
                compactor,
                reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
            },
            model: model.clone().erase(),
            meter: meter.clone(),
            turn_usage: turn_usage.clone(),
            session_usage: crate::hook::TurnUsage::default(),
            store_latch: Default::default(),
            pending_summary: Default::default(),
        },
    );
    Ok(TurnHook {
        hook,
        bus,
        meter,
        turn_usage,
    })
}

/// Drive one turn on `hook` against `model` and return the inference-completed
/// narration the hook emitted, if any.
async fn drive_turn(
    hook: HookChain<PolicyPermissionResolver>,
    bus: &Bus,
    model: MockCompletionModel,
) -> Result<Option<NarrationEvent>> {
    let mut rx = bus.narration().subscribe();
    let agent = rig::agent::AgentBuilder::new(model.erase())
        .add_hook(hook)
        .build();
    let mut stream = agent.prompt("run").max_turns(4).stream();
    while stream.next().await.is_some() {}

    while let Ok(event) = rx.try_recv() {
        if matches!(event, NarrationEvent::InferenceCompleted { .. }) {
            return Ok(Some(event));
        }
    }
    Ok(None)
}

/// Run one turn against `model` and return the inference-completed narration
/// the hook emitted, if any.
async fn emitted_completion(model: MockCompletionModel) -> Result<Option<NarrationEvent>> {
    emitted_completion_with(model.clone(), model).await
}

/// Run one turn against `model`, with `compactor_model` as the summarizer's
/// wire, and return the inference-completed narration the hook emitted, if any.
async fn emitted_completion_with(
    model: MockCompletionModel,
    compactor_model: MockCompletionModel,
) -> Result<Option<NarrationEvent>> {
    let TurnHook { hook, bus, .. } = turn_hook(&model, compactor_model)?;
    drive_turn(hook, &bus, model).await
}

/// Run one turn against `model` and return the inference-completed narration
/// the hook emitted.
async fn completed_event(model: MockCompletionModel) -> Result<NarrationEvent> {
    emitted_completion(model)
        .await?
        .ok_or_else(|| "the turn must emit InferenceCompleted".into())
}

/// Run one turn against `model` with `compactor_model` as the summarizer's
/// wire, and return the inference-completed narration the hook emitted.
async fn completed_event_with(
    model: MockCompletionModel,
    compactor_model: MockCompletionModel,
) -> Result<NarrationEvent> {
    emitted_completion_with(model, compactor_model)
        .await?
        .ok_or_else(|| "the turn must emit InferenceCompleted".into())
}

/// The cost and completeness the hook emitted.
fn emitted_cost(event: NarrationEvent) -> Result<(Option<f64>, bool)> {
    match event {
        NarrationEvent::InferenceCompleted { usage } => Ok((usage.cost, usage.cost_complete)),
        other => Err(format!("the turn must emit InferenceCompleted, got {other:?}").into()),
    }
}

/// The total the hook emitted, or `None` when the wire reported none.
fn emitted_total_tokens(event: NarrationEvent) -> Result<Option<u64>> {
    match event {
        NarrationEvent::InferenceCompleted { usage } => Ok(usage.total_tokens),
        other => Err(format!("the turn must emit InferenceCompleted, got {other:?}").into()),
    }
}

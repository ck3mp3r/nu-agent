use super::*;

use rig::catalog::Catalog;
use rig::completion::Usage;
use rig::test_utils::MockTurn;

/// A usage that reports both counters, so the catalog can price it.
fn usage() -> Usage {
    Usage::new()
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
fn catalog_turn(reference: &str, usage: Usage) -> Result<MockCompletionModel> {
    Ok(MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("done".to_string()),
        MockStreamEvent::final_response(usage),
    ]])
    .priced_as(catalog_spec(reference)?))
}

/// A scripted unary model answering as the catalog's `reference`, with `usage`
/// on its response.
fn catalog_call(reference: &str, usage: Usage) -> Result<MockCompletionModel> {
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
    last_total_tokens: Arc<Mutex<Option<u64>>>,
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
    let last_total_tokens = Arc::new(Mutex::new(None));
    let compactor = NuCompactor::new(
        compactor_model.erase(),
        Bus::default(),
        rig_memory::CompactionPolicy::default(),
        test_model_spec(),
    );
    let hook = HookChain::new(
        bus.clone(),
        PolicyPermissionResolver {
            permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
            session_grants: Arc::new(Mutex::new(crate::tools::authz::SessionGrantCache::default())),
            closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
            mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        },
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
            repetition_guard: false,
            memory,
            conversation_id: "cost-test-conv".to_string(),
            compaction: CompactionConfig {
                compactor,
                params: CompactionParams::default(),
                threshold_tokens: None,
                threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
            },
            model: model.clone().erase(),
            last_total_tokens: last_total_tokens.clone(),
        },
    );
    Ok(TurnHook {
        hook,
        bus,
        last_total_tokens,
    })
}

/// Drive one turn on `hook` against `model` and return the `LlmCompleted`
/// event the hook emitted, if any.
async fn drive_turn(
    hook: HookChain<PolicyPermissionResolver>,
    bus: &Bus,
    model: MockCompletionModel,
) -> Result<Option<UiEvent>> {
    let mut rx = bus.ui_event().subscribe();
    let agent = rig::agent::AgentBuilder::new(model.erase())
        .add_hook(hook)
        .build();
    let mut stream = agent.prompt("run").max_turns(4).stream();
    while stream.next().await.is_some() {}

    while let Ok(event) = rx.try_recv() {
        if matches!(event, UiEvent::LlmCompleted { .. }) {
            return Ok(Some(event));
        }
    }
    Ok(None)
}

/// Run one turn against `model` and return the `LlmCompleted` event the hook
/// emitted, if any.
async fn emitted_completion(model: MockCompletionModel) -> Result<Option<UiEvent>> {
    emitted_completion_with(model.clone(), model).await
}

/// Run one turn against `model`, with `compactor_model` as the summarizer's
/// wire, and return the `LlmCompleted` event the hook emitted, if any.
async fn emitted_completion_with(
    model: MockCompletionModel,
    compactor_model: MockCompletionModel,
) -> Result<Option<UiEvent>> {
    let TurnHook { hook, bus, .. } = turn_hook(&model, compactor_model)?;
    drive_turn(hook, &bus, model).await
}

/// Run one turn against `model` and return the `LlmCompleted` event the hook
/// emitted.
async fn completed_event(model: MockCompletionModel) -> Result<UiEvent> {
    emitted_completion(model)
        .await?
        .ok_or_else(|| "the turn must emit LlmCompleted".into())
}

/// Run one turn against `model` with `compactor_model` as the summarizer's
/// wire, and return the `LlmCompleted` event the hook emitted.
async fn completed_event_with(
    model: MockCompletionModel,
    compactor_model: MockCompletionModel,
) -> Result<UiEvent> {
    emitted_completion_with(model, compactor_model)
        .await?
        .ok_or_else(|| "the turn must emit LlmCompleted".into())
}

/// The cost and completeness the hook emitted.
fn emitted_cost(event: UiEvent) -> Result<(Option<f64>, bool)> {
    match event {
        UiEvent::LlmCompleted {
            cost,
            cost_complete,
            ..
        } => Ok((cost, cost_complete)),
        other => Err(format!("the turn must emit LlmCompleted, got {other:?}").into()),
    }
}

/// The total the hook emitted, or `None` when the wire reported none.
fn emitted_total_tokens(event: UiEvent) -> Result<Option<u64>> {
    match event {
        UiEvent::LlmCompleted { total_tokens, .. } => Ok(total_tokens),
        other => Err(format!("the turn must emit LlmCompleted, got {other:?}").into()),
    }
}

/// Copilot is one of rig's unpriced providers, so the driver leaves
/// `usage.cost` None while the catalog prices the row. The hook must compute
/// the cost from the wire's own catalog-bound spec.
#[tokio::test]
async fn cost_computed_from_the_wire_spec_when_the_endpoint_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the row's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A provider the driver prices reports its own cost, and the hook emits that
/// figure rather than recomputing one.
#[tokio::test]
async fn cost_reported_by_the_endpoint_is_emitted_unchanged() -> Result<()> {
    // -- Setup & Fixtures: the reported figure, from a plain call on the same
    // wire.
    let plain = catalog_call("anthropic/claude-sonnet-4-6", usage())?.erase();
    let reported = plain
        .call("run")
        .await
        .map_err(|e| format!("the plain call must succeed: {e}"))?
        .usage
        .cost
        .ok_or("a priced provider's reply must carry a cost")?
        .total;
    let model = catalog_turn("anthropic/claude-sonnet-4-6", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a reported cost must be emitted")?;
    assert!(
        (total - reported).abs() < 1e-12,
        "the emitted cost must be the reported one, got {total} against {reported}"
    );
    assert!(cost_complete, "the reported cost covers the whole usage");
    Ok(())
}

/// A row the catalog does not price stays tokens-only, so no figure is
/// invented for it.
#[tokio::test]
async fn cost_stays_tokens_only_for_an_unpriced_row() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/gpt-4o", usage())?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    assert_eq!(
        cost, None,
        "a row the catalog does not price must stay tokens-only"
    );
    assert!(!cost_complete, "an unpriced call is not a complete cost");
    Ok(())
}

/// A cache part with tokens but no listed rate leaves the total a lower bound,
/// so the emitted cost is marked incomplete.
#[tokio::test]
async fn cost_marks_an_unlisted_cache_rate_incomplete() -> Result<()> {
    // -- Setup & Fixtures: the row lists no cache-write rate.
    let model = catalog_turn(
        "copilot/gemini-3.5-flash",
        usage().cache_creation_input_tokens(100),
    )?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (900.0 * 1.5 + 500.0 * 9.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must leave out the unlisted cache part, got {total}"
    );
    assert!(
        !cost_complete,
        "a cache part with tokens but no listed rate makes the cost a lower bound"
    );
    Ok(())
}

/// A usage that reports both counters but no total is still priced. The
/// ollama-cloud OpenAI-compatible endpoint omits `total_tokens`, and rig passes
/// that absence through verbatim, so the total must never gate pricing.
#[tokio::test]
async fn cost_computed_when_the_usage_reports_no_total_tokens() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — input and output reported,
    // total absent. The row lists 3.00 input and 15.00 output per million.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        Usage::new().input_tokens(1_000).output_tokens(500),
    )?;

    // -- Exec
    let (cost, cost_complete) = emitted_cost(completed_event(model).await?)?;

    // -- Check
    let total = cost.ok_or("a priced row must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the row's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A usage that reports both counters but no total emits `total_tokens: None`,
/// so the TUI can leave its displayed count alone instead of collapsing it to
/// zero.
#[tokio::test]
async fn total_less_usage_emits_no_total_tokens() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — counters present, total
    // absent.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        Usage::new().input_tokens(1_000).output_tokens(500),
    )?;

    // -- Exec
    let total_tokens = emitted_total_tokens(completed_event(model).await?)?;

    // -- Check
    assert_eq!(
        total_tokens, None,
        "an absent total must reach the payload as None, not zero"
    );
    Ok(())
}

/// A usage that reports input and output but no total still stores the real
/// count: the switch gate and the threshold read `input + output`.
#[tokio::test]
async fn total_tokens_stored_from_the_counters_when_the_usage_reports_no_total() -> Result<()> {
    // -- Setup & Fixtures: the ollama-cloud shape — counters present, total
    // absent.
    let model = catalog_turn(
        "copilot/claude-sonnet-4.6",
        Usage::new().input_tokens(1_000).output_tokens(500),
    )?;
    let TurnHook {
        hook,
        bus,
        last_total_tokens,
    } = turn_hook(&model, pricing_less_summarizer())?;

    // -- Exec
    drive_turn(hook, &bus, model).await?;

    // -- Check
    let stored = *last_total_tokens.lock().expect("model mutex poisoned");
    assert_eq!(
        stored,
        Some(1_500),
        "a total-less usage must store input + output"
    );
    Ok(())
}

/// A usage that reports no counter at all emits nothing: the turn had no
/// measurable reply, so there is nothing to price or count.
#[tokio::test]
async fn no_event_when_the_usage_reports_no_counter() -> Result<()> {
    // -- Setup & Fixtures
    let model = catalog_turn("copilot/claude-sonnet-4.6", Usage::default())?;

    // -- Exec
    let event = emitted_completion(model).await?;

    // -- Check
    assert!(
        event.is_none(),
        "a usage that reports no counter must emit nothing"
    );
    Ok(())
}

/// The compactor's wire is not the turn's: a turn on a priced model is priced
/// at that model's rates, not at the summarizer's, which nothing prices.
#[tokio::test]
async fn cost_priced_with_the_turns_own_model() -> Result<()> {
    // -- Setup & Fixtures: the turn's wire is priced, the summarizer's is not.
    let model = catalog_turn("copilot/claude-sonnet-4.6", usage())?;

    // -- Exec
    let (cost, cost_complete) =
        emitted_cost(completed_event_with(model, pricing_less_summarizer()).await?)?;

    // -- Check
    let total = cost.ok_or("the turn's own priced model must yield a computed cost")?;
    let expected = (1_000.0 * 3.0 + 500.0 * 15.0) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the emitted cost must be the turn's model's rates times the tokens, got {total}"
    );
    assert!(
        cost_complete,
        "a row that lists every rate prices the whole usage"
    );
    Ok(())
}

/// A turn on a model nothing prices records no cost, even when the
/// summarizer's wire is priced — the hook must not invent a cost from it —
/// and the warning names the turn's model, not the summarizer's.
#[tokio::test]
async fn cost_stays_none_for_a_pricing_less_turn_model() -> Result<()> {
    // -- Setup & Fixtures: the turn's row lists no price, the summarizer's
    // does.
    let model = catalog_turn("copilot/gpt-4o", usage())?;
    let summarizer = catalog_turn("copilot/claude-sonnet-4.6", usage())?;
    let TurnHook { hook, bus, .. } = turn_hook(&model, summarizer)?;

    // -- Exec
    // The once-per-model gate is consumed by the read, so read the warning
    // before the turn runs.
    let warning = hook
        .unpriced_warning(PricingSkip::NoPricing)
        .ok_or("the first unpriced call must warn")?;
    let (cost, cost_complete) = emitted_cost(
        drive_turn(hook, &bus, model)
            .await?
            .ok_or("the turn must emit LlmCompleted")?,
    )?;

    // -- Check
    assert_eq!(
        cost, None,
        "a turn on a model nothing prices must stay tokens-only"
    );
    assert!(!cost_complete, "an unpriced call is not a complete cost");
    assert!(
        warning.contains("copilot/gpt-4o"),
        "the warning must name the turn's model, got {warning}"
    );
    Ok(())
}

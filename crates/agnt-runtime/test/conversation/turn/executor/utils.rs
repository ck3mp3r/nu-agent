//! Shared test utilities for turn execution tests.
//!
//! `MockResolver`, `CancellingTool`, `BusEventCollector`, and `test_config()`
//! are extracted here so both `executor_test.rs` and `journey_test.rs` can
//! share them without duplication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bus::{Bus, StrictSubscription, Subscription};
use crate::config::Config;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use agnt_kernel::NoticeKind;
use agnt_kernel::bus::{ControlCommand, LifecycleEvent, NarrationEvent, SessionRecord};
use rig::DynModel;
use rig::operation::Completion;
use rig::test_utils::MockCompletionModel;

// ---------------------------------------------------------------------------
// BusEventCollector — observes the four concerns a turn publishes on the bus
// ---------------------------------------------------------------------------

/// One observation a turn test folds: the concern it arrived on, and the row.
///
/// The tests observe the same concerns a surface does — the ordered record
/// stream, the narration text, the lifecycle spine, and the control intake —
/// so this enum names those four and nothing else.
#[derive(Debug)]
pub(super) enum TurnTestObservation {
    Record(SessionRecord),
    Narration(NarrationEvent),
    Lifecycle(LifecycleEvent),
    Control(ControlCommand),
}

impl TurnTestObservation {
    /// A warning notice's text, when this observation is one.
    pub(super) fn warning_text(&self) -> Option<&str> {
        match self {
            Self::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                text,
            }) => Some(text),
            _ => None,
        }
    }

    /// The reason a narration stop carries, when this observation is one.
    pub(super) fn stopped_reason(&self) -> Option<&str> {
        match self {
            Self::Narration(NarrationEvent::InferenceStopped { reason }) => Some(reason),
            _ => None,
        }
    }

    /// An inference chunk's delta and aggregated text, when this observation is one.
    pub(super) fn chunk_text(&self) -> Option<(&str, &str)> {
        match self {
            Self::Narration(NarrationEvent::InferenceChunk { delta, aggregated }) => {
                Some((delta, aggregated))
            }
            _ => None,
        }
    }
}

/// Collects the four concerns published on a `Bus` during a turn. Since core
/// no longer threads a render handle through the turn, tests observe events by
/// subscribing to the same channels the TUI/TTY renderers subscribe to.
pub(super) struct BusEventCollector {
    stream: Subscription<SessionRecord>,
    narration: Subscription<NarrationEvent>,
    lifecycle: StrictSubscription<LifecycleEvent>,
    control: StrictSubscription<ControlCommand>,
}

impl BusEventCollector {
    pub(super) fn subscribe(bus: &Bus) -> Self {
        Self {
            stream: bus.stream().subscribe(),
            narration: bus.narration().subscribe(),
            lifecycle: bus.lifecycle().subscribe(),
            control: bus.control().subscribe(),
        }
    }

    /// Drain all subscribed concerns. Each concern keeps its own publication
    /// order; the order of the four groups is the concern order the bus
    /// declares (records, narration, lifecycle, control).
    pub(super) fn drain(&mut self) -> Vec<TurnTestObservation> {
        let mut observations = Vec::new();
        drain_ring(
            &mut self.stream,
            &mut observations,
            TurnTestObservation::Record,
        );
        drain_ring(
            &mut self.narration,
            &mut observations,
            TurnTestObservation::Narration,
        );
        drain_strict(
            &mut self.lifecycle,
            &mut observations,
            TurnTestObservation::Lifecycle,
        );
        drain_strict(
            &mut self.control,
            &mut observations,
            TurnTestObservation::Control,
        );
        observations
    }
}

/// Drain a bounded ring, resyncing at the tail when the consumer lagged.
fn drain_ring<T: Clone>(
    rx: &mut Subscription<T>,
    observations: &mut Vec<TurnTestObservation>,
    wrap: fn(T) -> TurnTestObservation,
) {
    loop {
        match rx.try_recv() {
            Ok(item) => observations.push(wrap(item)),
            Err(crate::bus::TryReceiveError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
}

/// Drain a never-dropped fan-out; a strict handle never lags.
fn drain_strict<T>(
    rx: &mut StrictSubscription<T>,
    observations: &mut Vec<TurnTestObservation>,
    wrap: fn(T) -> TurnTestObservation,
) {
    while let Ok(item) = rx.try_recv() {
        observations.push(wrap(item));
    }
}

// ---------------------------------------------------------------------------
// CancellingTool — a tool that publishes a CancelEvent after its first call
// ---------------------------------------------------------------------------

/// A `Tool` that publishes `CancelEvent::Requested` on the shared bus after
/// producing its result. This is the deterministic cancellation trigger used
/// by tests: the model must emit a `ToolCall` to this tool, and when the tool
/// runs (inside the turn's async context, after the hook has subscribed to
/// `bus.cancel()`) it publishes the cancel that terminates the turn.
pub struct CancellingTool {
    pub bus: Bus,
    fired: Arc<AtomicBool>,
}

impl CancellingTool {
    pub fn new(bus: Bus) -> Self {
        Self {
            bus,
            fired: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl rig::tool::Tool for CancellingTool {
    const NAME: &'static str = "test_cancel_tool";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Tool that cancels the turn after its first invocation".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        // Publish the cancel only on the first invocation so the tool result is
        // recorded before the turn terminates.
        if !self.fired.swap(true, Ordering::SeqCst) {
            tokio::task::yield_now().await;
            let _ = self
                .bus
                .cancel()
                .send(crate::bus::CancelEvent::Requested)
                .await;
        }
        Ok("cancelling_tool_result".to_string())
    }
}

// ---------------------------------------------------------------------------
// Compaction test defaults
// ---------------------------------------------------------------------------

/// A `NuCompactor` (no marker store) with a deterministic streaming mock model
/// so compaction never invokes a real LLM.
pub(super) fn test_compactor(bus: Bus) -> crate::conversation::compaction::compactor::NuCompactor {
    use crate::conversation::compaction::compactor::NuCompactor;
    let turns: Vec<Vec<rig::test_utils::MockStreamEvent>> = (0..8)
        .map(|_| {
            vec![
                rig::test_utils::MockStreamEvent::Text("summary".to_string()),
                rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    NuCompactor::from_shared_model(
        Arc::new(std::sync::Mutex::new(model.erase())),
        bus,
        rig_memory::CompactionPolicy::default(),
        test_model_spec(),
    )
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

/// A `CompactionConfig` with deterministic defaults (no LLM invoked) used by
/// tests that are not exercising compaction.
pub(super) fn test_compaction_config(
    bus: Bus,
) -> crate::conversation::compaction::CompactionConfig {
    crate::conversation::compaction::CompactionConfig {
        compactor: test_compactor(bus.clone()),
        threshold_tokens: None,
        threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
    }
}

// ---------------------------------------------------------------------------
// Shared model handle helper
// ---------------------------------------------------------------------------

/// Build a shared `Arc<Mutex<DynModel<Completion>>>` wrapping a deterministic
/// mock model.
pub(super) fn shared_mock_model() -> Arc<std::sync::Mutex<DynModel<Completion>>> {
    Arc::new(std::sync::Mutex::new(
        MockCompletionModel::text("summary").erase(),
    ))
}

/// Wrap a specific `MockCompletionModel` in a shared
/// `Arc<Mutex<DynModel<Completion>>>`.
pub(super) fn shared_model_handle(
    model: MockCompletionModel,
) -> Arc<std::sync::Mutex<DynModel<Completion>>> {
    Arc::new(std::sync::Mutex::new(model.erase()))
}

// ---------------------------------------------------------------------------
// MockResolver — always allows
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(super) struct MockResolver;

impl AsyncPermissionResolver for MockResolver {
    fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _stream: &crate::bus::Stream,
        _facts: &crate::bus::FactsTx,
        _cancel: &crate::bus::CancelTx,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = PermissionDecision::Allow;
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// test_config
// ---------------------------------------------------------------------------

/// Build a minimal Config for testing.
///
/// Uses `max_retries: Some(0)` so existing tests that exercise error persistence paths
/// are not affected by the retry loop. Tests that specifically verify retry behaviour
/// should override this with `max_retries: Some(3)` (or the desired count).
pub(super) fn test_config() -> Config {
    Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        catalog: None,
        model: "gpt-4o".to_string(),
        api_key: None,
        base_url: None,
        preamble: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tokens: None,
        max_tool_turns: None,
        temperature: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
        pricing: None,
    }
}

/// Extract the first Text content string from a User or Assistant message.
pub(crate) fn message_text(msg: &crate::types::Message) -> Option<String> {
    match msg {
        crate::types::Message::User { content } => content.iter().find_map(|c| {
            if let crate::types::UserContent::Text(t) = c {
                Some(t.text.clone())
            } else {
                None
            }
        }),
        crate::types::Message::Assistant(assistant) => {
            let content = &assistant.content;
            content.iter().find_map(|c| {
                if let crate::types::AssistantContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
        }
        crate::types::Message::System { .. } => None,
    }
}

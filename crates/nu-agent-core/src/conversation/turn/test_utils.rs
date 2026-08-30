//! Shared test utilities for turn execution tests.
//!
//! `MockResolver`, `CancellingTool`, `BusEventCollector`, and `test_config()`
//! are extracted here so both `executor_test.rs` and `journey_test.rs` can
//! share them without duplication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bus::{Bus, CompactionRx, LlmRx, PermissionRx, ToolRx, TurnRx, WarningRx};
use crate::compaction::CompactionParams;
use crate::config::Config;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::protocol::event::UiEvent;
use rig::agent::ModelHandle;
use rig::test_utils::MockCompletionModel;

// ---------------------------------------------------------------------------
// BusEventCollector — collects UiEvents from the shared bus during a turn
// ---------------------------------------------------------------------------

/// Collects lifecycle events published on a `Bus` during a turn and converts
/// them to `UiEvent` at the boundary. Since core no longer threads a
/// `ProgressUi` through the turn, tests observe events by subscribing to the
/// same channels the TUI/TTY renderers subscribe to.
pub(super) struct BusEventCollector {
    tool_rx: ToolRx,
    llm_rx: LlmRx,
    turn_rx: TurnRx,
    warning_rx: WarningRx,
    compaction_rx: CompactionRx,
    permission_rx: PermissionRx,
}

impl BusEventCollector {
    pub(super) fn subscribe(bus: &Bus) -> Self {
        Self {
            tool_rx: bus.tool().subscribe(),
            llm_rx: bus.llm().subscribe(),
            turn_rx: bus.turn().subscribe(),
            warning_rx: bus.warning().subscribe(),
            compaction_rx: bus.compaction().subscribe(),
            permission_rx: bus.permission().subscribe(),
        }
    }

    fn drain_channel<T>(rx: &mut crate::bus::BroadcastRx<T>, events: &mut Vec<UiEvent>)
    where
        Option<UiEvent>: From<T>,
        T: Clone + Send + 'static,
    {
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if let Some(e) = Option::<UiEvent>::from(event) {
                        events.push(e);
                    }
                }
                Err(crate::bus::TryRecvError::Empty) => break,
                Err(crate::bus::TryRecvError::Lagged(_)) => continue,
                Err(crate::bus::TryRecvError::Closed) => break,
            }
        }
    }

    /// Drain all subscribed channels, converting events to `UiEvent` in
    /// insertion order (tool, llm, turn, warning, compaction, permission).
    pub(super) fn drain(&mut self) -> Vec<UiEvent> {
        let mut events = Vec::new();
        Self::drain_channel(&mut self.tool_rx, &mut events);
        Self::drain_channel(&mut self.llm_rx, &mut events);
        Self::drain_channel(&mut self.turn_rx, &mut events);
        Self::drain_channel(&mut self.warning_rx, &mut events);
        Self::drain_channel(&mut self.compaction_rx, &mut events);
        Self::drain_channel(&mut self.permission_rx, &mut events);
        events
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

/// A `NuCompactor<FsSessionStore>` (no marker store) with a deterministic
/// streaming mock model so compaction never invokes a real LLM.
pub(super) fn test_compactor(
    bus: Bus,
) -> crate::conversation::compaction::compactor::NuCompactor<crate::session::FsSessionStore> {
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
        Arc::new(std::sync::Mutex::new(ModelHandle::new(model))),
        bus,
        None,
    )
}

/// A `CompactionConfig<FsSessionStore>` with deterministic defaults (no LLM
/// invoked) used by tests that are not exercising compaction.
pub(super) fn test_compaction_config(
    bus: Bus,
) -> crate::conversation::compaction::CompactionConfig<crate::session::FsSessionStore> {
    crate::conversation::compaction::CompactionConfig {
        compactor: test_compactor(bus.clone()),
        params: CompactionParams::default(),
        threshold_tokens: None,
    }
}

// ---------------------------------------------------------------------------
// Shared model handle helper
// ---------------------------------------------------------------------------

/// Build a shared `Arc<Mutex<ModelHandle>>` wrapping a deterministic mock model.
pub(super) fn shared_mock_model_handle() -> Arc<std::sync::Mutex<ModelHandle>> {
    Arc::new(std::sync::Mutex::new(ModelHandle::new(
        MockCompletionModel::text("summary"),
    )))
}

/// Wrap a specific `MockCompletionModel` in a shared `Arc<Mutex<ModelHandle>>`.
pub(super) fn shared_model_handle(
    model: MockCompletionModel,
) -> Arc<std::sync::Mutex<ModelHandle>> {
    Arc::new(std::sync::Mutex::new(ModelHandle::new(model)))
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
        _bus: &crate::bus::Bus,
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
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    }
}

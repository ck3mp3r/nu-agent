//! Shared test utilities for turn execution tests.
//!
//! `MockUi`, `MockResolver`, and `test_config()` are extracted here so both
//! `executor_test.rs` and `journey_test.rs` can share them without duplication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bus::Bus;
use crate::config::Config;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// MockUi
// ---------------------------------------------------------------------------

pub(super) struct MockUi {
    pub events: Vec<UiEvent>,
    cancel_flag: Arc<AtomicBool>,
    cancel_bus: Option<Bus>,
    cancel_published: Arc<AtomicBool>,
}

impl MockUi {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            cancel_bus: None,
            cancel_published: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Pre-set cancel so the first `take_cancel_requested()` fires and publishes
    /// a `CancelEvent` to the bus. This drives cancellation through the shared
    /// bus channel (which the hook and tool proxies subscribe to) instead of a
    /// standalone flag, matching the production flow.
    pub fn immediately_cancelled(bus: Bus) -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(true)),
            cancel_bus: Some(bus),
            cancel_published: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns a `MockUi` and the `Bus` used to cancel the running turn.
    ///
    /// Call `bus.cancel().send(CancelEvent::Requested { .. })` at any point during
    /// the running turn to cancel it from outside — including from within a mock
    /// tool's `call()` implementation.
    pub fn with_external_cancel() -> (Self, Bus) {
        let bus = crate::bus::create_bus();
        let ui = Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            cancel_bus: None,
            cancel_published: Arc::new(AtomicBool::new(false)),
        };
        (ui, bus)
    }
}

impl ProgressUi for MockUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        let was_cancelled = self.cancel_flag.swap(false, Ordering::SeqCst);
        if was_cancelled
            && !self.cancel_published.swap(true, Ordering::SeqCst)
            && let Some(bus) = &self.cancel_bus
        {
            let _ = bus
                .cancel()
                .send(crate::bus::CancelEvent::Requested { task_id: None });
        }
        was_cancelled
    }
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
        _ui_tx: Option<mpsc::UnboundedSender<UiEvent>>,
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

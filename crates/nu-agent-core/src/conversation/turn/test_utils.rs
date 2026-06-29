//! Shared test utilities for turn execution tests.
//!
//! `MockUi`, `MockResolver`, and `test_config()` are extracted here so both
//! `executor_test.rs` and `journey_test.rs` can share them without duplication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::Config;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

// ---------------------------------------------------------------------------
// MockUi
// ---------------------------------------------------------------------------

pub(super) struct MockUi {
    pub events: Vec<UiEvent>,
    cancel_flag: Arc<AtomicBool>,
    /// An externally-managed cancellation token injected into the turn executor.
    /// When `Some`, the turn executor uses this token instead of creating a fresh one,
    /// so callers can cancel the running turn at any point (including from inside a
    /// mock tool's `call()` implementation).
    external_token: Option<tokio_util::sync::CancellationToken>,
}

impl MockUi {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            external_token: None,
        }
    }

    /// Pre-set cancel so take_cancel_requested() fires on the very first drain
    /// loop iteration — causes cancel_token to be set before the spawned tokio
    /// task processes any stream event, which makes build_agent_and_stream return
    /// Ok(StreamingTurnResult { cancelled: true, messages: Some(chat_history) }).
    pub fn immediately_cancelled() -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(true)),
            external_token: None,
        }
    }

    /// Returns a `MockUi` and the `CancellationToken` it will inject into the turn executor.
    ///
    /// Call `token.cancel()` at any point during the running turn to cancel it from outside —
    /// including from within a mock tool's `call()` implementation. The token is injected via
    /// `external_cancel_token()` which the turn executor reads in place of creating a fresh one.
    ///
    /// This avoids the 16ms drain-loop sleep race inherent in setting a cancel flag via `emit()`.
    pub fn with_external_cancel() -> (Self, tokio_util::sync::CancellationToken) {
        let token = tokio_util::sync::CancellationToken::new();
        let ui = Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            external_token: Some(token.clone()),
        };
        (ui, token)
    }
}

impl ProgressUi for MockUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_flag.swap(false, Ordering::SeqCst)
    }

    fn external_cancel_token(&self) -> Option<tokio_util::sync::CancellationToken> {
        self.external_token.clone()
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
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = PermissionDecision::Allow;
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// test_config
// ---------------------------------------------------------------------------

/// Build a minimal Config for testing.
pub(super) fn test_config() -> Config {
    Config {
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
    }
}

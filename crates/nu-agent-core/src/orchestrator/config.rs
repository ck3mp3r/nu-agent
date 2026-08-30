//! Configuration types for the interactive orchestrator loop.

use std::sync::Arc;

use nu_agent_a2a::{A2aCompletionEvent, IncomingTask};
use nu_protocol::Span;
use tokio::sync::mpsc;

use crate::bus::Bus;
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::OrchestratorEvent;
use crate::protocol::contracts::UiMessageSnapshot;

/// Configuration for the interactive loop.
///
/// Groups common arguments that would otherwise be passed individually,
/// keeping function signatures under clippy's `too_many_arguments` threshold.
pub struct InteractiveLoopConfig<F = fn(mpsc::Sender<OrchestratorEvent>)> {
    /// The span to use for values created during the loop.
    pub span: Span,
    /// Pending permission requests awaiting user decisions.
    pub interactive_pending: Option<PendingPermissions>,
    /// Optional channel for receiving task IDs to cancel (e.g., A2A tasks).
    pub task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Optional channel for receiving incoming A2A tasks directly.
    pub a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    /// Optional channel for receiving A2A completion events directly.
    pub a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
    /// Shared cancellation bus.
    pub bus: Bus,
    /// Optional hydration config for resuming a prior session.
    pub hydration: Option<HydrationConfig>,
    /// Optional callback invoked after a successful agent switch.
    /// Receives the new agent's identity (name) and optional description.
    /// Used by the binary layer to update the A2A agent card.
    pub on_agent_switch: Option<OnAgentSwitch>,
    /// Optional closure that spawns the TUI render loop.
    /// Receives the orchestrator event sender.
    pub spawn_render_loop: Option<F>,
}

impl InteractiveLoopConfig<fn(mpsc::Sender<OrchestratorEvent>)> {
    /// Create a new config with the given span and all other fields set to `None`.
    pub fn new(span: Span) -> Self {
        Self {
            span,
            interactive_pending: None,
            task_cancel_rx: None,
            a2a_task_rx: None,
            a2a_completion_rx: None,
            bus: crate::bus::create_bus(),
            hydration: None,
            on_agent_switch: None,
            spawn_render_loop: None,
        }
    }
}

impl<F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static> InteractiveLoopConfig<F> {
    /// Set the interactive pending permissions.
    pub fn with_interactive_pending(mut self, pending: Option<PendingPermissions>) -> Self {
        self.interactive_pending = pending;
        self
    }

    /// Set the task cancel receiver.
    pub fn with_task_cancel_rx(mut self, rx: Option<mpsc::UnboundedReceiver<String>>) -> Self {
        self.task_cancel_rx = rx;
        self
    }

    /// Set the incoming A2A task receiver.
    pub fn with_a2a_task_rx(mut self, rx: Option<mpsc::Receiver<IncomingTask>>) -> Self {
        self.a2a_task_rx = rx;
        self
    }

    /// Set the A2A completion event receiver.
    pub fn with_a2a_completion_rx(
        mut self,
        rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
    ) -> Self {
        self.a2a_completion_rx = rx;
        self
    }

    /// Set the shared cancellation bus.
    pub fn with_bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self
    }

    /// Set the hydration config using a builder pattern.
    pub fn with_hydration(
        mut self,
        messages: Vec<UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) -> Self {
        self.hydration = Some(HydrationConfig {
            messages,
            last_total_tokens,
        });
        self
    }

    /// Set the on-agent-switch callback.
    ///
    /// The callback receives the new agent's identity (name) and optional
    /// description after a successful switch. Used by the binary layer to
    /// update the A2A agent card.
    pub fn with_on_agent_switch(mut self, callback: OnAgentSwitch) -> Self {
        self.on_agent_switch = Some(callback);
        self
    }

    /// Set the render-loop spawner closure.
    pub fn with_spawn_render_loop<F2: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static>(
        self,
        f: F2,
    ) -> InteractiveLoopConfig<F2> {
        let InteractiveLoopConfig {
            span,
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            bus,
            hydration,
            on_agent_switch,
            spawn_render_loop: _,
        } = self;
        InteractiveLoopConfig {
            span,
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            bus,
            hydration,
            on_agent_switch,
            spawn_render_loop: Some(f),
        }
    }
}

/// Configuration for hydrating a prior session into the interactive loop.
pub struct HydrationConfig {
    /// Messages from the prior session to display in the transcript.
    pub messages: Vec<UiMessageSnapshot>,
    /// The total token count from the prior session, if known.
    pub last_total_tokens: Option<u64>,
}

/// Callback invoked after a successful agent switch.
/// Receives the new agent's identity (name), optional description, and optional icon.
pub type OnAgentSwitch = Arc<dyn Fn(String, Option<String>, Option<String>) + Send + Sync>;

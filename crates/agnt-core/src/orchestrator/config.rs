//! Configuration types for the interactive orchestrator loop.

use std::sync::Arc;

use agnt_a2a::{A2aCompletionEvent, InMemoryTaskStore, IncomingTask};
use tokio::sync::mpsc;

use crate::conversation::runtime::PendingPermissions;
use agnt_runtime::bus::Bus;
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::contracts::UiMessageSnapshot;

/// Configuration for the orchestrator loop.
///
/// Every seam has a typed default, so [`LoopConfig::default`] takes nothing and
/// the `with_…` builders are the only way to set one.
pub struct LoopConfig<F = fn(mpsc::Sender<OrchestratorEvent>), H = NoAgentSwitchHook> {
    /// Pending permission requests awaiting user decisions.
    interactive_pending: Option<PendingPermissions>,
    /// Optional channel for receiving task IDs to cancel (e.g., A2A tasks).
    task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Optional channel for receiving incoming A2A tasks directly.
    a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    /// Optional channel for receiving A2A completion events directly.
    a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
    /// Optional A2A task store. When present, tasks that arrive while the
    /// worker is busy are rejected or failed instead of dispatched.
    task_store: Option<Arc<InMemoryTaskStore>>,
    /// Shared cancellation bus.
    bus: Bus,
    /// Optional hydration config for resuming a prior session.
    hydration: Option<HydrationConfig>,
    /// Optional hook invoked after a successful agent switch.
    /// Receives the new agent's identity (name), optional description, and
    /// optional icon. Used by the binary layer to update the A2A agent card.
    on_agent_switch: Option<H>,
    /// Optional sink for the loop's event stream. Receives the orchestrator
    /// event sender; the drawing surface subscribes through it.
    event_sink: Option<F>,
}

impl Default for LoopConfig<fn(mpsc::Sender<OrchestratorEvent>), NoAgentSwitchHook> {
    /// A configuration with a fresh bus and every seam unset.
    fn default() -> Self {
        Self {
            interactive_pending: None,
            task_cancel_rx: None,
            a2a_task_rx: None,
            a2a_completion_rx: None,
            task_store: None,
            bus: agnt_runtime::bus::Bus::default(),
            hydration: None,
            on_agent_switch: None,
            event_sink: None,
        }
    }
}

impl<F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static, H> LoopConfig<F, H> {
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

    /// Set the A2A task store.
    pub fn with_task_store(mut self, store: Option<Arc<InMemoryTaskStore>>) -> Self {
        self.task_store = store;
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

    /// Set the on-agent-switch hook.
    ///
    /// The hook receives the new agent's identity (name), optional description,
    /// and optional icon after a successful switch. Used by the binary layer to
    /// update the A2A agent card.
    pub fn with_on_agent_switch<H2: AgentSwitchHook>(self, hook: H2) -> LoopConfig<F, H2> {
        let LoopConfig {
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            task_store,
            bus,
            hydration,
            on_agent_switch: _,
            event_sink,
        } = self;
        LoopConfig {
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            task_store,
            bus,
            hydration,
            on_agent_switch: Some(hook),
            event_sink,
        }
    }

    /// Set the loop's event sink.
    pub fn with_event_sink<F2: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static>(
        self,
        f: F2,
    ) -> LoopConfig<F2, H> {
        let LoopConfig {
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            task_store,
            bus,
            hydration,
            on_agent_switch,
            event_sink: _,
        } = self;
        LoopConfig {
            interactive_pending,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
            task_store,
            bus,
            hydration,
            on_agent_switch,
            event_sink: Some(f),
        }
    }
}

/// The seams a loop consumes from a [`LoopConfig`].
///
/// The configuration keeps its fields private, so consumption goes through
/// [`LoopConfig::into_parts`] and the loop reads a named view instead of a
/// field pattern.
pub(crate) struct LoopParts<F, H> {
    pub(crate) interactive_pending: Option<PendingPermissions>,
    pub(crate) task_store: Option<Arc<InMemoryTaskStore>>,
    pub(crate) bus: Bus,
    pub(crate) hydration: Option<HydrationConfig>,
    pub(crate) on_agent_switch: Option<H>,
    pub(crate) event_sink: Option<F>,
}

/// The control intake a loop owns for one run.
///
/// These are the receivers the loop reads on its own select arms, so they leave
/// the configuration at [`LoopConfig::into_parts`] and are never read from a
/// config view again.
pub(crate) struct LoopIntake {
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub(crate) a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    pub(crate) a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
}

impl<F, H> LoopConfig<F, H> {
    /// Consume the configuration into the seams the loop reads and the intake
    /// it owns.
    pub(crate) fn into_parts(self) -> (LoopParts<F, H>, LoopIntake) {
        let parts = LoopParts {
            interactive_pending: self.interactive_pending,
            task_store: self.task_store,
            bus: self.bus,
            hydration: self.hydration,
            on_agent_switch: self.on_agent_switch,
            event_sink: self.event_sink,
        };
        let intake = LoopIntake {
            task_cancel_rx: self.task_cancel_rx,
            a2a_task_rx: self.a2a_task_rx,
            a2a_completion_rx: self.a2a_completion_rx,
        };
        (parts, intake)
    }
}

/// Configuration for hydrating a prior session into the interactive loop.
pub struct HydrationConfig {
    /// Messages from the prior session to display in the transcript.
    pub messages: Vec<UiMessageSnapshot>,
    /// The total token count from the prior session, if known.
    pub last_total_tokens: Option<u64>,
}

/// Hook invoked after a successful agent switch.
///
/// Receives the new agent's identity (name), optional description, and optional
/// icon. Implementors are statically dispatched through
/// [`LoopConfig`].
pub trait AgentSwitchHook: Send + Sync + 'static {
    /// Called after a switch succeeded, with the switched-to agent's details.
    fn on_agent_switch(
        &self,
        agent_identity: String,
        agent_description: Option<String>,
        agent_icon: Option<String>,
    );
}

/// The default hook: does nothing.
///
/// Used as the type-parameter default so construction sites that never set a
/// hook keep compiling unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoAgentSwitchHook;

impl AgentSwitchHook for NoAgentSwitchHook {
    fn on_agent_switch(
        &self,
        _agent_identity: String,
        _agent_description: Option<String>,
        _agent_icon: Option<String>,
    ) {
    }
}

//! Configuration types for the interactive orchestrator loop.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::external::{ExternalCompletion, ExternalTask, ExternalTaskStore, NoExternalTaskStore};
use crate::bus::Bus;
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::events::OrchestratorEvent;
use crate::protocol::contracts::UiMessageSnapshot;

/// Configuration for the orchestrator loop.
///
/// Every seam has a typed default, so [`LoopConfig::default`] takes nothing and
/// the `with_…` builders are the only way to set one.
pub struct LoopConfig<
    F = fn(mpsc::Sender<OrchestratorEvent>),
    H = NoAgentSwitchHook,
    S = NoExternalTaskStore,
> {
    /// Pending permission requests awaiting user decisions.
    interactive_pending: Option<PendingPermissions>,
    /// Optional channel for receiving task IDs to cancel (e.g., A2A tasks).
    task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Optional channel for receiving runtime-typed external tasks.
    external_task_rx: Option<mpsc::Receiver<ExternalTask>>,
    /// Optional channel for receiving runtime-typed external completions.
    external_completion_rx: Option<mpsc::Receiver<ExternalCompletion>>,
    /// Optional store for rejecting external tasks that cannot run. Tasks that
    /// arrive while the worker is busy are rejected through it.
    external_task_store: Option<Arc<S>>,
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
            external_task_rx: None,
            external_completion_rx: None,
            external_task_store: None,
            bus: crate::bus::Bus::default(),
            hydration: None,
            on_agent_switch: None,
            event_sink: None,
        }
    }
}

impl<F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static, H, S> LoopConfig<F, H, S> {
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

    /// Set the runtime-typed external task receiver.
    pub fn with_external_task_rx(mut self, rx: Option<mpsc::Receiver<ExternalTask>>) -> Self {
        self.external_task_rx = rx;
        self
    }

    /// Set the runtime-typed external completion receiver.
    pub fn with_external_completion_rx(
        mut self,
        rx: Option<mpsc::Receiver<ExternalCompletion>>,
    ) -> Self {
        self.external_completion_rx = rx;
        self
    }

    /// Set the runtime-typed external task store.
    ///
    /// The store's type replaces the configuration's `S` parameter, the same
    /// way `with_event_sink` replaces `F` and `with_on_agent_switch` replaces
    /// `H`.
    pub fn with_external_task_store<S2: ExternalTaskStore + 'static>(
        self,
        store: Option<Arc<S2>>,
    ) -> LoopConfig<F, H, S2> {
        let LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store: _,
            bus,
            hydration,
            on_agent_switch,
            event_sink,
        } = self;
        LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store: store,
            bus,
            hydration,
            on_agent_switch,
            event_sink,
        }
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
    pub fn with_on_agent_switch<H2: AgentSwitchHook>(self, hook: H2) -> LoopConfig<F, H2, S> {
        let LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store,
            bus,
            hydration,
            on_agent_switch: _,
            event_sink,
        } = self;
        LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store,
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
    ) -> LoopConfig<F2, H, S> {
        let LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store,
            bus,
            hydration,
            on_agent_switch,
            event_sink: _,
        } = self;
        LoopConfig {
            interactive_pending,
            task_cancel_rx,
            external_task_rx,
            external_completion_rx,
            external_task_store,
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
pub(crate) struct LoopParts<F, H, S> {
    pub(crate) interactive_pending: Option<PendingPermissions>,
    pub(crate) external_task_store: Option<Arc<S>>,
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
    pub(crate) external_task_rx: Option<mpsc::Receiver<ExternalTask>>,
    pub(crate) external_completion_rx: Option<mpsc::Receiver<ExternalCompletion>>,
}

impl<F, H, S> LoopConfig<F, H, S> {
    /// Consume the configuration into the seams the loop reads and the intake
    /// it owns.
    pub(crate) fn into_parts(self) -> (LoopParts<F, H, S>, LoopIntake) {
        let parts = LoopParts {
            interactive_pending: self.interactive_pending,
            external_task_store: self.external_task_store,
            bus: self.bus,
            hydration: self.hydration,
            on_agent_switch: self.on_agent_switch,
            event_sink: self.event_sink,
        };
        let intake = LoopIntake {
            task_cancel_rx: self.task_cancel_rx,
            external_task_rx: self.external_task_rx,
            external_completion_rx: self.external_completion_rx,
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

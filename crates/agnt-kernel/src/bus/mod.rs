//! The kernel bus: six concerns, one contract each.
//!
//! | concern | payload | delivery |
//! |---|---|---|
//! | [`stream`](Bus::stream) | `SessionRecord` | ordered, lossy under lag |
//! | [`cancel`](Bus::cancel) | `CancelEvent` | subscribe-before-publish, never dropped |
//! | [`control`](Bus::control) | `ControlCommand` | never dropped, ordered |
//! | [`lifecycle`](Bus::lifecycle) | `LifecycleEvent` | ordered, complete |
//! | [`narration`](Bus::narration) | `NarrationEvent` | ordered, oldest-droppable tail |
//! | [`facts`](Bus::facts) | `FactRecord` | latest-wins, droppable |
//!
//! The generic mechanisms are private: the public consumable surface is the
//! concern types themselves, named for behavior. Capacities live here, beside
//! the concerns, so the whole delivery contract is readable in one place.

// region:    --- Modules

mod cancel;
mod channel;
mod control;
mod events;
mod facts;
mod lifecycle;
mod narration;
mod ring;
mod stream;

pub use crate::records::SessionRecord;
pub use cancel::Cancel;
pub use channel::{
    AgentRx, AgentTx, BroadcastRx, BroadcastTx, CancelRx, CancelTx, ChannelError, ChannelResult,
    ControlRx, ControlTx, FactsRx, FactsTx, LifecycleRx, LifecycleTx, Metrics, MpscRx, MpscTx,
    NarrationRx, NarrationTx, NoMetrics, OneshotRx, OneshotTx, TryRecvError,
};
pub use control::Control;
pub use events::{
    AgentEvent, CancelEvent, ControlCommand, FactRecord, LifecycleEvent, McpServerUsability,
    NarrationEvent, PermissionGate, PromptStamps, SharedUiAction, WorkerPhase,
};
pub use facts::Facts;
pub use lifecycle::Lifecycle;
pub use narration::Narration;
pub use ring::{
    PublishError, ReceiveError, Strict, StrictSubscription, Subscription, TryReceiveError,
};
pub use stream::Stream;

// endregion: --- Modules

// region:    --- Types

/// The kernel bus: the composition root of the six concerns.
#[derive(Clone)]
pub struct Bus {
    stream: Stream,
    cancel: Cancel,
    control: Control,
    lifecycle: Lifecycle,
    narration: Narration,
    facts: Facts,
}

impl Bus {
    /// A bus with the standard capacities: stream 1024, cancel 64, narration
    /// 1024, facts 256; control and lifecycle are uncapped.
    pub fn new() -> Self {
        Self {
            stream: Stream::new(),
            cancel: Cancel::new(),
            control: Control::new(),
            lifecycle: Lifecycle::new(),
            narration: Narration::new(),
            facts: Facts::new(),
        }
    }

    /// The record stream concern: the ordered session log.
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    /// The strict cancel concern.
    pub fn cancel(&self) -> &Cancel {
        &self.cancel
    }

    /// The control concern: commands addressed to the loop owner.
    pub fn control(&self) -> &Control {
        &self.control
    }

    /// The lifecycle concern: the ordered, complete turn spine.
    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// The narration concern: text for a human to read.
    pub fn narration(&self) -> &Narration {
        &self.narration
    }

    /// The facts concern: latest-wins snapshots of kernel state.
    pub fn facts(&self) -> &Facts {
        &self.facts
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

// endregion: --- Types

//! The kernel bus: six concerns, one contract each.
//!
//! | concern | payload | delivery |
//! |---|---|---|
//! | [`stream`](Stream) | `SessionRecord` | ordered, lossy under lag — event lane and text lane |
//! | [`cancel`](Cancel) | `CancelEvent` | subscribe-before-publish, never dropped |
//! | [`control`](Control) | `ControlCommand` | never dropped, ordered |
//! | [`lifecycle`](Lifecycle) | `LifecycleEvent` | ordered, complete |
//! | [`narration`](Narration) | `NarrationEvent` | ordered, oldest-droppable tail |
//! | [`facts`](Facts) | `FactRecord` | latest-wins, droppable |
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
    AgentEvent, CancelEvent, CommandFacts, CompactionAvailability, ControlCommand, FactRecord,
    LifecycleEvent, McpServerUsability, NarrationEvent, PermissionGate, PromptStamps,
    SharedUiAction, WorkerPhase,
};
pub use facts::Facts;
pub use lifecycle::Lifecycle;
pub use narration::Narration;
pub use ring::{
    PublishError, ReceiveError, Strict, StrictSubscription, Subscription, TryReceiveError,
};
pub use stream::Stream;

// endregion: --- Modules

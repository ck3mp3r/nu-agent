//! The bus: the Level-2 channel aliases, the typed hub and the Level-3 domain
//! methods over those aliases.

// region:    --- Modules

mod domain;
mod hub;

pub use agnt_kernel::bus::{
    AgentEvent, AgentRx, AgentTx, BroadcastRx, BroadcastTx, CancelEvent, CancelRx, CancelTx,
    ChannelError, ChannelResult, Control, ControlCommand, ControlRx, ControlTx, FactRecord, Facts,
    FactsRx, FactsTx, Lifecycle, LifecycleEvent, LifecycleRx, LifecycleTx, McpServerUsability,
    MpscRx, MpscTx, Narration, NarrationEvent, NarrationRx, NarrationTx, OneshotRx, OneshotTx,
    PublishError, ReceiveError, SharedUiAction, Stream, StrictSubscription, Subscription,
    TryReceiveError, TryRecvError,
};
pub use domain::{
    PermissionCard, RequestCancel, auto_resolve_permission, request_permission, settle_permission,
};
pub use hub::Bus;

// endregion: --- Modules

#[cfg(test)]
#[path = "../../test/bus/domain.rs"]
mod domain_test;

#[cfg(test)]
#[path = "../../test/bus/test.rs"]
mod test;

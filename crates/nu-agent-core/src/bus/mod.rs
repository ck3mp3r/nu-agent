mod bus_impl;
mod channel;
mod domain;
mod events;

#[cfg(test)]
#[path = "channel_test.rs"]
mod channel_test;
#[cfg(test)]
#[path = "domain_test.rs"]
mod domain_test;
#[cfg(test)]
#[path = "event_from_test.rs"]
mod event_from_test;
#[cfg(test)]
mod test;

pub use bus_impl::{Bus, create_bus};
pub use channel::{
    BroadcastRx, BroadcastTx, CancelRx, CancelTx, ChannelError, ChannelResult, CompactionRx,
    CompactionTx, ExternalRx, ExternalTx, LlmRx, LlmTx, MpscRx, MpscTx, OneshotRx, OneshotTx,
    PermissionRx, PermissionTx, SessionRx, SessionTx, ToolRx, ToolTx, TryRecvError, TurnRx, TurnTx,
    UiStateRx, UiStateTx, WarningRx, WarningTx,
};
pub use events::{
    CancelEvent, CompactionEvent, ExternalEvent, LlmEvent, PermissionEvent, SessionEvent,
    ToolEvent, TurnEvent, WarningEvent,
};

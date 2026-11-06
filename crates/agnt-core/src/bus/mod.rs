mod channel;
#[cfg(test)]
#[path = "../../test/bus/channel.rs"]
mod channel_test;
#[cfg(test)]
#[path = "../../test/bus/domain.rs"]
mod domain_test;
#[cfg(test)]
#[path = "../../test/bus/test.rs"]
mod test;

mod domain;
mod hub;

pub use agnt_kernel::bus::{
    AgentEvent, AgentRx, AgentTx, BroadcastRx, BroadcastTx, CancelEvent, CancelRx, CancelTx,
    ChannelError, ChannelResult, ExternalEvent, ExternalRx, ExternalTx, MpscRx, MpscTx, OneshotRx,
    OneshotTx, SessionEvent, SessionRx, SessionTx, TryRecvError, TurnEvent, TurnRx, TurnTx,
};
pub use channel::{CompactionRx, CompactionTx, UiEventRx, UiEventTx, UiStateRx, UiStateTx};
pub use domain::{RequestCancel, RequestPermission};
pub use hub::{Bus, create_bus};

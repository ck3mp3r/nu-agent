mod channel;
#[cfg(test)]
#[path = "../../test/bus/channel.rs"]
mod channel_test;
#[cfg(test)]
#[path = "../../test/bus/domain.rs"]
mod domain_test;
#[cfg(test)]
#[path = "../../test/bus/event_from.rs"]
mod event_from_test;
#[cfg(test)]
#[path = "../../test/bus/test.rs"]
mod test;

mod domain;
mod events;
mod hub;

pub use channel::{
    AgentRx, AgentTx, BroadcastRx, BroadcastTx, CancelRx, CancelTx, ChannelError, ChannelResult,
    CompactionRx, CompactionTx, ExternalRx, ExternalTx, MpscRx, MpscTx, OneshotRx, OneshotTx,
    SessionRx, SessionTx, TryRecvError, TurnRx, TurnTx, UiEventRx, UiEventTx, UiStateRx, UiStateTx,
};
pub use events::{
    AgentEvent, CancelEvent, CompactionEvent, ExternalEvent, LlmEvent, PermissionEvent,
    SessionEvent, ToolEvent, TurnEvent, WarningEvent,
};
pub use hub::{Bus, create_bus};

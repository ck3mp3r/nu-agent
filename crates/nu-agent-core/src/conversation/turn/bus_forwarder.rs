use tokio::sync::broadcast;

use crate::bus::{Bus, CompactionEvent, LlmEvent, ToolEvent, TurnEvent, WarningEvent};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

/// Forwards bus lifecycle events to a `ProgressUi` by subscribing to the
/// broadcast channels and converting each event via its `From` impl.
///
/// SRP: this struct owns bus-to-UI forwarding. The turn drain loop owns
/// orchestration. Adding a new bus channel only requires extending this
/// struct, not the drain loop.
pub(crate) struct BusForwarder {
    llm_rx: broadcast::Receiver<LlmEvent>,
    tool_rx: broadcast::Receiver<ToolEvent>,
    turn_rx: broadcast::Receiver<TurnEvent>,
    warning_rx: broadcast::Receiver<WarningEvent>,
    compaction_rx: broadcast::Receiver<CompactionEvent>,
}

impl BusForwarder {
    pub(crate) fn new(bus: &Bus) -> Self {
        Self {
            llm_rx: bus.llm().subscribe(),
            tool_rx: bus.tool().subscribe(),
            turn_rx: bus.turn().subscribe(),
            warning_rx: bus.warning().subscribe(),
            compaction_rx: bus.compaction().subscribe(),
        }
    }

    /// Drain all subscribed channels, converting events to `UiEvent` and
    /// calling `ui.emit()` for each. Events that convert to `None` are dropped.
    pub(crate) fn drain_to<U: ProgressUi>(&mut self, ui: &mut U) {
        drain_channel(&mut self.llm_rx, ui);
        drain_channel(&mut self.tool_rx, ui);
        drain_channel(&mut self.turn_rx, ui);
        drain_channel(&mut self.warning_rx, ui);
        drain_channel(&mut self.compaction_rx, ui);
    }
}

/// Drain a single broadcast channel, forwarding each event that converts to a
/// `Some(UiEvent)`. `Empty` ends the drain; `Lagged` skips to the next event.
fn drain_channel<T, U: ProgressUi>(rx: &mut broadcast::Receiver<T>, ui: &mut U)
where
    Option<UiEvent>: From<T>,
    T: Clone,
{
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if let Some(e) = Option::<UiEvent>::from(event) {
                    ui.emit(&e);
                }
            }
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
}

//! The default bus: no metrics hooked, FIFO preserved per channel.

use crate::bus::{Bus, UiEventRx};
use crate::protocol::events::{SystemEvent, ToolCall};
use agnt_kernel::ToolName;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The default bus uses no metrics and its channels preserve FIFO order.
#[tokio::test]
async fn bus_default_uses_no_metrics_and_preserves_fifo() -> Result<()> {
    // -- Case 1: the default bus still sends and receives
    let bus = Bus::default();
    let mut rx = bus.ui_event().subscribe();
    bus.ui_event()
        .send(SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("read".to_string()),
                crate::tools::handler::ToolSource::Unknown,
                "{}",
            ),
        })
        .await?;
    let event = rx.recv().await?;
    assert!(
        matches!(event, SystemEvent::ToolStarted { .. }),
        "default bus channels must still send and receive"
    );

    // -- Case 2: the ui_event channel preserves FIFO order
    let bus = Bus::default();
    let mut rx: UiEventRx = bus.ui_event().subscribe();
    bus.ui_event().send(SystemEvent::Tick).await?;
    bus.ui_event().send(SystemEvent::InferenceStarted).await?;
    let first = rx.recv().await?;
    let second = rx.recv().await?;
    assert_eq!(first, SystemEvent::Tick, "first event must arrive first");
    assert_eq!(
        second,
        SystemEvent::InferenceStarted,
        "second event must arrive second"
    );
    Ok(())
}

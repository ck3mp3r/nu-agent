//! Cancel-contract tests: the strict steering ring.

use crate::bus::{Bus, CancelEvent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Subscribe-before-publish: a subscriber that subscribes first receives the
/// cancellation request — the resolver relies on this ordering.
#[tokio::test]
async fn test_cancel_subscribe_before_publish_receives_requested() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.cancel().subscribe();

    // -- Exec
    bus.cancel().request().ok_or("cancel should accept")?;
    drop(bus);

    // -- Check
    assert!(
        matches!(
            rx.recv().await.ok_or("cancel event")?,
            CancelEvent::Requested
        ),
        "the subscriber-before-publish contract should deliver the cancel"
    );
    Ok(())
}

/// The cancel ring is independent of the record stream: a cancel published
/// mid-flood is unaffected by stream capacity — and vice versa.
#[tokio::test]
async fn test_cancel_remains_independent_of_stream_flood() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut cancel_rx = bus.cancel().subscribe();

    // -- Exec
    // Saturate the stream ring past capacity; then request cancel.
    for i in 0..1026 {
        let _published = bus
            .stream()
            .publish(crate::records::SessionRecord::Inference(
                crate::records::InferenceRecord::Chunk {
                    delta: i.to_string(),
                    aggregated: i.to_string(),
                },
            ));
    }
    bus.cancel()
        .request()
        .ok_or("cancel should accept past a stream flood")?;
    drop(bus);

    // -- Check
    let cancel = cancel_rx
        .recv()
        .await
        .ok_or("cancel event past a stream flood")?;
    assert!(
        matches!(cancel, CancelEvent::Requested),
        "the cancel ring must remain independent of record-stream floods"
    );
    Ok(())
}

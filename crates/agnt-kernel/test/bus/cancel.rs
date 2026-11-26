//! Cancel-contract tests: the strict steering ring.

use crate::bus::{Cancel, CancelEvent, Stream};
use crate::records::{InferenceId, InferenceRecord, SessionRecord};

use crate::test_support::Result;

/// Subscribe-before-publish: a subscriber that subscribes first receives the
/// cancellation request — the resolver relies on this ordering.
#[tokio::test]
async fn test_cancel_subscribe_before_publish_receives_requested() -> Result<()> {
    // -- Setup & Fixtures
    let cancel = Cancel::new();
    let mut rx = cancel.subscribe();

    // -- Exec
    cancel
        .request()
        .map_err(|e| format!("cancel should accept: {e:?}"))?;
    drop(cancel);

    // -- Check
    let event = rx
        .recv()
        .await
        .map_err(|e| format!("cancel event: {e:?}"))?;
    assert!(
        matches!(event, CancelEvent::Requested),
        "the subscriber-before-publish contract should deliver the cancel"
    );
    Ok(())
}

/// The cancel ring is independent of the record stream: a cancel published
/// mid-flood is unaffected by stream capacity — and vice versa.
#[tokio::test]
async fn test_cancel_remains_independent_of_stream_flood() -> Result<()> {
    // -- Setup & Fixtures
    let cancel = Cancel::new();
    let stream = Stream::new();
    let mut cancel_rx = cancel.subscribe();

    // -- Exec
    // Saturate the stream ring past capacity; then request cancel.
    for i in 0..1026 {
        let _published = stream.publish(SessionRecord::Inference(InferenceRecord::Chunk {
            inference: InferenceId::mint(),
            delta: i.to_string(),
            aggregated: i.to_string(),
        }));
    }
    cancel
        .request()
        .map_err(|e| format!("cancel should accept past a stream flood: {e:?}"))?;
    drop(cancel);
    drop(stream);

    // -- Check
    let event = cancel_rx
        .recv()
        .await
        .map_err(|e| format!("cancel event past a stream flood: {e:?}"))?;
    assert!(
        matches!(event, CancelEvent::Requested),
        "the cancel ring must remain independent of record-stream floods"
    );
    Ok(())
}

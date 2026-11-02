//! Stream-contract tests: ordering, lag resync, zero retention.

use crate::bus::Bus;
use crate::records::{InferenceRecord, SessionRecord, Usage};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Publish order == receive order across record kinds.
#[tokio::test]
async fn test_stream_delivers_records_in_publish_order() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Started))
        .ok_or("stream should accept with a subscriber")?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .ok_or("stream should accept")?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: crate::records::Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
            },
        }))
        .ok_or("stream should accept")?;
    drop(bus);

    // -- Check
    let first = rx.recv().await.ok_or("first record")?;
    assert!(
        matches!(first, SessionRecord::Inference(InferenceRecord::Started)),
        "the first record should be Inference::Started"
    );
    let second = rx.recv().await.ok_or("second record")?;
    assert!(
        matches!(&second, SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) if aggregated == "a"),
        "the second record should be Inference::Chunk with the accumulated text"
    );
    assert!(
        matches!(
            rx.recv().await.ok_or("third record")?,
            SessionRecord::Inference(InferenceRecord::Completed { .. })
        ),
        "the third record should be Inference::Completed"
    );
    Ok(())
}

/// The ring is delivery, not a log: publishing with zero subscribers reports
/// the send failure instead of silently dropping.
#[tokio::test]
async fn test_stream_publish_without_subscribers_reports_error() {
    // -- Setup & Fixtures
    let bus = Bus::new();

    // -- Exec & Check
    let result = bus
        .stream()
        .publish(SessionRecord::Inference(InferenceRecord::Started));
    assert!(
        result.is_err(),
        "a zero-subscriber ring should report the send failure"
    );
}

/// Lag contract: a consumer stalled past capacity sees `Lagged`, stays alive,
/// loses only older records, and re-syncs at the tail in publish order.
#[tokio::test]
async fn test_stream_lagged_consumer_resyncs_at_tail() -> Result<()> {
    // -- Setup & Fixtures
    // The stream ring capacity is 1024: 1026 publishes guarantee the
    // subscriber misses exactly the first two records.
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    for i in 0..1026 {
        let record = SessionRecord::Inference(InferenceRecord::Chunk {
            delta: i.to_string(),
            aggregated: i.to_string(),
        });
        let _published = bus.stream().publish(record);
    }
    drop(bus);
    let mut seen: Vec<String> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match record {
                SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) => {
                    seen.push(aggregated)
                }
                other => panic!("unexpected non-chunk record while draining: {other:?}"),
            },
            Err(crate::bus::ReceiveError::Lagged(_)) => continue,
            Err(crate::bus::ReceiveError::Closed) => break,
        }
    }

    // -- Check
    assert!(
        seen.len() >= 1022,
        "should re-sync with the tail, got {} records",
        seen.len()
    );
    let first: u64 = seen[0].parse()?;
    assert!(
        first >= 2,
        "records before the lag boundary must be gone, saw {first} first"
    );
    let last: u64 = seen.last().ok_or("tail record")?.parse()?;
    assert_eq!(last, 1025, "the newest record must survive the lag");
    for pair in seen.windows(2) {
        let a: u64 = pair[0].parse()?;
        let b: u64 = pair[1].parse()?;
        assert!(
            a < b,
            "tail delivery must stay in publish order, saw {a} then {b}"
        );
    }
    Ok(())
}

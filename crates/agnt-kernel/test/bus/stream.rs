//! Stream-contract tests: ordering, lag resync, zero retention.

use crate::bus::{Bus, ReceiveError};
use crate::records::{InferenceRecord, SessionRecord, TurnRecord, Usage};

use crate::test_support::Result;

/// Publish order == receive order across record kinds.
#[tokio::test]
async fn test_stream_delivers_records_in_publish_order() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Started))
        .map_err(|e| format!("stream should accept with a subscriber: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .map_err(|e| format!("stream should accept: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
            },
        }))
        .map_err(|e| format!("stream should accept: {e:?}"))?;
    drop(bus);

    // -- Check
    let first = rx
        .recv()
        .await
        .map_err(|e| format!("first record: {e:?}"))?;
    assert!(
        matches!(first, SessionRecord::Inference(InferenceRecord::Started)),
        "the first record should be Inference::Started"
    );
    let second = rx
        .recv()
        .await
        .map_err(|e| format!("second record: {e:?}"))?;
    assert!(
        matches!(&second, SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) if aggregated == "a"),
        "the second record should be Inference::Chunk with the accumulated text"
    );
    let third = rx
        .recv()
        .await
        .map_err(|e| format!("third record: {e:?}"))?;
    assert!(
        matches!(
            third,
            SessionRecord::Inference(InferenceRecord::Completed { .. })
        ),
        "the third record should be Inference::Completed"
    );
    Ok(())
}

/// The ring is delivery, not a log: publishing with zero subscribers reports
/// the send failure instead of silently dropping.
#[test]
fn test_stream_publish_without_subscribers_reports_error() {
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
            Err(ReceiveError::Lagged(_)) => continue,
            Err(ReceiveError::Closed) => break,
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

/// Vocabulary producer contract: within one call, chunks accumulate
/// monotonically and usage arrives before the turn record. Migrates beside
/// the producer once the turn-executor lifts into the kernel.
#[tokio::test]
async fn test_stream_vocabulary_orders_usage_before_turn_completion() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Started))
        .map_err(|e| format!("publish started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .map_err(|e| format!("publish chunk a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "b".to_string(),
            aggregated: "ab".to_string(),
        }))
        .map_err(|e| format!("publish chunk b: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
            },
        }))
        .map_err(|e| format!("publish completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 0,
            duration_ms: 2,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut aggregate: Option<String> = None;
    let mut usage_seen = false;
    let mut turn_seen_at: Option<usize> = None;
    let mut index: usize = 0;
    loop {
        match rx.recv().await {
            Ok(record) => {
                match record {
                    SessionRecord::Inference(InferenceRecord::Started) => {}
                    SessionRecord::Inference(InferenceRecord::Stopped { .. }) => {}
                    SessionRecord::Tool(_) => {}
                    SessionRecord::Permission(_) => {}
                    SessionRecord::Notice(_) => {}
                    SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) => {
                        assert!(
                            aggregate
                                .as_deref()
                                .map(|prev| aggregated.starts_with(prev))
                                .unwrap_or(true),
                            "aggregated must accumulate monotonically"
                        );
                        aggregate = Some(aggregated);
                    }
                    SessionRecord::Inference(InferenceRecord::Completed { .. }) => {
                        usage_seen = true
                    }
                    SessionRecord::Turn(TurnRecord::Completed { .. }) => turn_seen_at = Some(index),
                }
                index += 1;
            }
            Err(ReceiveError::Closed) => break,
            Err(ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert!(usage_seen, "usage must be narrated");
    let turn_index = turn_seen_at.ok_or("the turn record must arrive")?;
    assert!(
        index > turn_index && usage_seen,
        "usage must precede the turn record"
    );
    assert_eq!(
        aggregate.as_deref(),
        Some("ab"),
        "the final aggregate must be the full text"
    );
    Ok(())
}

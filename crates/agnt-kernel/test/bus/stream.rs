//! Stream-contract tests: ordering on each lane, lag resync, zero retention,
//! and the text lane's independence from the event lane.

use crate::bus::{ReceiveError, Stream};
use crate::records::{
    CompactionRecord, CompactionSource, InferenceId, InferenceRecord, SessionRecord, SummaryStage,
    TurnRecord, Usage,
};

use crate::test_support::Result;

/// Publish order == receive order on the event lane, and the text lane carries
/// the per-delta records on its own.
#[tokio::test]
async fn test_stream_delivers_records_in_publish_order() -> Result<()> {
    // -- Setup & Fixtures
    let stream = Stream::new();
    let mut rx = stream.subscribe();
    let mut text_rx = stream.subscribe_text();

    // -- Exec
    stream
        .publish(SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }))
        .map_err(|e| format!("stream should accept with a subscriber: {e:?}"))?;
    stream
        .publish_text(SessionRecord::Inference(InferenceRecord::Chunk {
            inference: InferenceId::mint(),
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .map_err(|e| format!("the text lane should accept: {e:?}"))?;
    stream
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
                cost: None,
                cost_complete: false,
            },
        }))
        .map_err(|e| format!("stream should accept: {e:?}"))?;
    drop(stream);

    // -- Check: the event lane carries the discrete records alone, in order
    let first = rx
        .recv()
        .await
        .map_err(|e| format!("first record: {e:?}"))?;
    assert!(
        matches!(
            first,
            SessionRecord::Inference(InferenceRecord::Started { .. })
        ),
        "the first record should be Inference::Started"
    );
    let second = rx
        .recv()
        .await
        .map_err(|e| format!("second record: {e:?}"))?;
    assert!(
        matches!(
            second,
            SessionRecord::Inference(InferenceRecord::Completed { .. })
        ),
        "the second record should be Inference::Completed: a published chunk never rides the event lane"
    );
    assert!(
        rx.recv().await.is_err(),
        "the event lane must carry nothing else"
    );

    // -- Check: the text lane carries the chunk the producer published there
    let chunk = text_rx
        .recv()
        .await
        .map_err(|e| format!("the text record: {e:?}"))?;
    assert!(
        matches!(&chunk, SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) if aggregated == "a"),
        "the text lane should carry the chunk with the accumulated text"
    );
    Ok(())
}

/// Lane independence: WHEN a producer publishes far past the text lane's
/// capacity while an event subscriber is not reading, THEN the event subscriber
/// SHALL still receive every event in publication order.
///
/// The burst carries both kinds of per-delta record — an implementation's
/// chunks and a compaction summary's chunks — because both are text, and both
/// compete with the event lane for capacity if either is published there.
#[tokio::test]
async fn test_stream_text_burst_never_costs_an_event() -> Result<()> {
    // -- Setup & Fixtures
    let stream = Stream::new();
    let mut rx = stream.subscribe();
    let mut text_rx = stream.subscribe_text();
    let chunk = SessionRecord::Inference(InferenceRecord::Chunk {
        inference: InferenceId::mint(),
        delta: "d".to_string(),
        aggregated: "d".to_string(),
    });
    let summary_chunk = SessionRecord::Compaction(CompactionRecord::SummaryChunk {
        source: CompactionSource::Auto,
        stage: SummaryStage::History,
        delta: "s".to_string(),
        aggregated: "s".to_string(),
    });

    // -- Exec: the event subscriber reads none of the burst, and the burst is
    // ten times the ring's own capacity.
    for index in 0..10_240 {
        let record = if index % 2 == 0 {
            chunk.clone()
        } else {
            summary_chunk.clone()
        };
        let _ = stream.publish_text(record);
    }
    for index in 0..64 {
        let _ = stream.publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: index,
        }));
        let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
            source: Some(CompactionSource::Auto),
            summary: format!("summary {index}"),
        }));
    }
    drop(stream);

    // -- Check: every event arrives, in order, with no lag
    for index in 0..64 {
        let record = rx
            .recv()
            .await
            .map_err(|e| format!("event {index}: {e:?}"))?;
        match record {
            SessionRecord::Turn(TurnRecord::Completed { duration_ms }) => {
                assert_eq!(duration_ms, index, "the event lane must keep publish order");
            }
            other => return Err(format!("unexpected record on the event lane: {other:?}").into()),
        }
        let record = rx
            .recv()
            .await
            .map_err(|e| format!("compaction event {index}: {e:?}"))?;
        match record {
            SessionRecord::Compaction(CompactionRecord::Completed { summary, .. }) => {
                assert_eq!(
                    summary,
                    format!("summary {index}"),
                    "the event lane must keep publish order"
                );
            }
            other => return Err(format!("unexpected record on the event lane: {other:?}").into()),
        }
    }

    // -- Check: the text lane spent its own capacity, and its tail survives
    let mut tail = None;
    loop {
        match text_rx.recv().await {
            Ok(_) => tail = Some(()),
            Err(ReceiveError::Lagged(_)) => continue,
            Err(ReceiveError::Closed) => break,
        }
    }
    assert!(
        tail.is_some(),
        "the text lane should deliver its surviving tail"
    );
    Ok(())
}

/// The ring is delivery, not a log: publishing with zero subscribers reports
/// the send failure instead of silently dropping.
#[test]
fn test_stream_publish_without_subscribers_reports_error() {
    // -- Setup & Fixtures
    let stream = Stream::new();

    // -- Exec & Check
    let result = stream.publish(SessionRecord::Inference(InferenceRecord::Started {
        inference: InferenceId::mint(),
    }));
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
    let stream = Stream::new();
    let mut rx = stream.subscribe_text();

    // -- Exec
    for i in 0..1026 {
        let record = SessionRecord::Inference(InferenceRecord::Chunk {
            inference: InferenceId::mint(),
            delta: i.to_string(),
            aggregated: i.to_string(),
        });
        let _published = stream.publish_text(record);
    }
    drop(stream);
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
/// monotonically on the text lane, and usage arrives before the turn record on
/// the event lane. Migrates beside the producer once the turn-executor lifts
/// into the kernel.
#[tokio::test]
async fn test_stream_vocabulary_orders_usage_before_turn_completion() -> Result<()> {
    // -- Setup & Fixtures
    let stream = Stream::new();
    let mut rx = stream.subscribe();
    let mut text_rx = stream.subscribe_text();

    // -- Exec
    stream
        .publish(SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }))
        .map_err(|e| format!("publish started: {e:?}"))?;
    stream
        .publish_text(SessionRecord::Inference(InferenceRecord::Chunk {
            inference: InferenceId::mint(),
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .map_err(|e| format!("publish chunk a: {e:?}"))?;
    stream
        .publish_text(SessionRecord::Inference(InferenceRecord::Chunk {
            inference: InferenceId::mint(),
            delta: "b".to_string(),
            aggregated: "ab".to_string(),
        }))
        .map_err(|e| format!("publish chunk b: {e:?}"))?;
    stream
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
                cost: None,
                cost_complete: false,
            },
        }))
        .map_err(|e| format!("publish completed: {e:?}"))?;
    stream
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 2,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(stream);

    // -- Check: the text lane's aggregates accumulate monotonically
    let mut aggregate: Option<String> = None;
    loop {
        match text_rx.recv().await {
            Ok(SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. })) => {
                assert!(
                    aggregate
                        .as_deref()
                        .map(|prev| aggregated.starts_with(prev))
                        .unwrap_or(true),
                    "aggregated must accumulate monotonically"
                );
                aggregate = Some(aggregated);
            }
            Ok(other) => {
                return Err(format!("the text lane carries chunks alone, not {other:?}").into());
            }
            Err(ReceiveError::Closed) => break,
            Err(ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        aggregate.as_deref(),
        Some("ab"),
        "the final aggregate must be the full text"
    );

    // -- Check: usage precedes the turn record on the event lane
    let mut usage_seen = false;
    let mut turn_seen_at: Option<usize> = None;
    let mut index: usize = 0;
    loop {
        match rx.recv().await {
            Ok(record) => {
                match record {
                    SessionRecord::Inference(InferenceRecord::Started { .. }) => {}
                    SessionRecord::Inference(InferenceRecord::Stopped { .. }) => {}
                    SessionRecord::Tool(_) => {}
                    SessionRecord::Permission(_) => {}
                    SessionRecord::Prompt(_) => {}
                    SessionRecord::Notice(_) => {}
                    SessionRecord::Compaction(_) => {}
                    SessionRecord::Session(_) => {}
                    SessionRecord::Inference(InferenceRecord::Completed { .. }) => {
                        usage_seen = true
                    }
                    SessionRecord::Turn(TurnRecord::Completed { .. }) => turn_seen_at = Some(index),
                    SessionRecord::Inference(InferenceRecord::Chunk { .. }) => {
                        panic!("chunks never ride the event lane")
                    }
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
    Ok(())
}

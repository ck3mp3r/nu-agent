//! Record-vocabulary contract tests: producer promises of the session log.
//!
//! The vehicle is the bus stream; the contracts belong to `records.rs` —
//! these pin what every producer (turn executor, compactor) must emit.

use crate::bus::Bus;
use crate::records::{InferenceRecord, SessionRecord, TurnRecord, Usage};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Within one call: chunks accumulate monotonically and usage arrives before
/// the turn record. Every producer of [`SessionRecord`]s must honor this.
#[tokio::test]
async fn test_records_single_call_orders_usage_before_turn_completion() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Started))
        .ok_or("publish started")?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .ok_or("publish chunk a")?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Chunk {
            delta: "b".to_string(),
            aggregated: "ab".to_string(),
        }))
        .ok_or("publish chunk b")?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: Some(3),
            },
        }))
        .ok_or("publish completed")?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 0,
            duration_ms: 2,
        }))
        .ok_or("publish turn")?;
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
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
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

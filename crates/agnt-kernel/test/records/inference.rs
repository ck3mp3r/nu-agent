//! Inference and turn narration contracts: the sequence promises every
//! producer of [`SessionRecord`]s must honor.

use crate::bus::Bus;
use crate::records::{InferenceRecord, SessionRecord, StopCause, TurnRecord, Usage};

use crate::test_support::Result;

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

/// The strict narration sequence of a streaming text turn: `Started` opens
/// the window, chunks accumulate monotonically, `Completed{usage}` closes
/// the call, and `Turn::Completed` closes the turn exactly once, last.
#[tokio::test]
async fn test_records_streaming_turn_pins_full_narration_sequence() -> Result<()> {
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
        .map_err(|e| format!("publish chunk ab: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: Some(5),
            },
        }))
        .map_err(|e| format!("publish completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 7,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut aggregates: Vec<String> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Inference(InferenceRecord::Started) => tags.push("started"),
                SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) => {
                    tags.push("chunk");
                    aggregates.push(aggregated.clone());
                }
                SessionRecord::Inference(InferenceRecord::Completed { .. }) => {
                    tags.push("completed")
                }
                SessionRecord::Inference(InferenceRecord::Stopped { .. }) => tags.push("stopped"),
                SessionRecord::Tool(_) => tags.push("tool"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
            },
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    for window in aggregates.windows(2) {
        assert!(
            window[1].len() >= window[0].len(),
            "aggregated must accumulate monotonically"
        );
    }
    assert_eq!(
        tags,
        ["started", "chunk", "chunk", "completed", "turn"],
        "the streaming-turn narration sequence is pinned"
    );
    Ok(())
}

/// A hook-stopped call narrates `Stopped{cause}` instead of `Completed`; the
/// cause is an enum and the reason is content; the turn itself still closes —
/// the producer promise of `executor/dispatch.rs:241/252 → :263`.
#[tokio::test]
async fn test_records_stopped_call_narrates_stop_before_turn_completion() -> Result<()> {
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
        .publish(SessionRecord::Inference(InferenceRecord::Stopped {
            cause: StopCause::OutputRepetition,
            reason: "the assistant kept repeating the same output".to_string(),
        }))
        .map_err(|e| format!("publish stopped: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 4,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut stop_causes: Vec<StopCause> = Vec::new();
    let mut stop_reasons: Vec<String> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Inference(InferenceRecord::Started) => tags.push("started"),
                SessionRecord::Inference(InferenceRecord::Chunk { .. }) => tags.push("chunk"),
                SessionRecord::Inference(InferenceRecord::Completed { .. }) => {
                    tags.push("completed")
                }
                SessionRecord::Inference(InferenceRecord::Stopped { cause, reason }) => {
                    tags.push("stopped");
                    stop_causes.push(*cause);
                    stop_reasons.push(reason.clone());
                }
                SessionRecord::Tool(_) => tags.push("tool"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
            },
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        tags,
        ["started", "chunk", "stopped", "turn"],
        "a stopped call narrates stop, never completion, and the turn still closes"
    );
    assert_eq!(
        stop_causes,
        [StopCause::OutputRepetition],
        "the stop cause is an enumerated producer fact, never sniffed from text"
    );
    assert_eq!(
        stop_reasons,
        ["the assistant kept repeating the same output"],
        "the stop reason must travel with the record"
    );
    Ok(())
}

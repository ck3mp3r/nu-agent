//! Record-vocabulary contract tests: producer promises of the session log.
//!
//! The vehicle is the bus stream; the contracts belong to `records.rs` —
//! these pin what every producer (turn executor, compactor) must emit.

use crate::bus::Bus;
use crate::records::{
    InferenceRecord, SessionRecord, StopCause, ToolCall, ToolOutcome, ToolRecord, ToolStatus,
    TurnRecord, Usage,
};

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
/// This is the sequence the turn-executor producer suite must reproduce
/// when the producer lifts onto the kernel (fabric-gap parity test).
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
            tool_calls: 0,
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

/// A hook-stopped call narrates `Stopped` instead of `Completed`; the stop
/// reason travels with the record; the turn itself still closes — the
/// producer promise of `executor/dispatch.rs:241/252 → :263`.
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
            tool_calls: 0,
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

/// The tool windows of a turn: each dispatched call narrates `Started` then
/// exactly one `Completed`, in stream order, correlated by the derived call
/// id — the anchor consumers key on, never rendered text.
#[tokio::test]
async fn test_records_tool_windows_narrate_started_then_completed_per_call() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let first = ToolCall::new("nu__shell", "builtin", r#"{"command":"ls"}"#);
    let second = ToolCall::new("fs__read", "builtin", r#"{"path":"a.txt"}"#);
    assert_ne!(first.id, second.id, "distinct calls must key distinctly");

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: first.clone(),
        }))
        .map_err(|e| format!("publish tool started a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: first.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                result: "a.txt".to_string(),
                message: None,
            },
        }))
        .map_err(|e| format!("publish tool completed a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: second.clone(),
        }))
        .map_err(|e| format!("publish tool started b: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: second.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Failed,
                result: String::new(),
                message: Some("no such file".to_string()),
            },
        }))
        .map_err(|e| format!("publish tool completed b: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 2,
            duration_ms: 9,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut completed: Vec<(crate::records::ToolCallId, ToolStatus)> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { call, outcome }) => {
                    tags.push("tool_completed");
                    completed.push((call.id.clone(), outcome.status));
                }
                SessionRecord::Inference(_) => tags.push("inference"),
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
        [
            "tool_started",
            "tool_completed",
            "tool_started",
            "tool_completed",
            "turn"
        ],
        "each call brackets as started then completed, in stream order"
    );
    assert_eq!(
        completed,
        vec![
            (first.id.clone(), ToolStatus::Success),
            (second.id.clone(), ToolStatus::Failed)
        ],
        "the completed records carry the call id and the enumerated disposition"
    );
    Ok(())
}

/// A permission-denied call still narrates the pair and settles as the
/// enumerated `Cancelled` disposition; the denial reason is content, not a
/// status (`session/tool_status.rs` law mirrored by the kernel).
#[tokio::test]
async fn test_records_denied_call_settles_cancelled_without_running() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let call = ToolCall::new("nu__shell", "builtin", r#"{"command":"rm -rf /"}"#);

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: call.clone(),
        }))
        .map_err(|e| format!("publish tool started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: call.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Cancelled,
                result: String::new(),
                message: Some("denied by policy".to_string()),
            },
        }))
        .map_err(|e| format!("publish tool completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 0,
            duration_ms: 3,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut denied: Vec<(ToolStatus, Option<String>)> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { outcome, .. }) => {
                    tags.push("tool_completed");
                    denied.push((outcome.status, outcome.message.clone()));
                }
                SessionRecord::Inference(_) => tags.push("inference"),
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
        ["tool_started", "tool_completed", "turn"],
        "a denied call still narrates its full pair before the turn closes"
    );
    assert_eq!(
        denied,
        vec![(ToolStatus::Cancelled, Some("denied by policy".to_string()))],
        "denial settles as the enumerated disposition; the reason is content"
    );
    Ok(())
}

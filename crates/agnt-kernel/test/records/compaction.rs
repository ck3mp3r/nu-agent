//! Compaction narration contracts: the summary sweep bracket and its failure.

use crate::bus::Bus;
use crate::records::{CompactionRecord, CompactionSource, SessionRecord, TurnRecord};

use crate::test_support::Result;

/// A compaction that runs brackets its summary stream: requested, started,
/// chunks with monotonic aggregation, completed with the summary content — and
/// the turn still closes.
#[tokio::test]
async fn test_records_compaction_brackets_summary_stream() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Requested {
            source: CompactionSource::Auto,
        }))
        .map_err(|e| format!("publish requested: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Started {
            source: CompactionSource::Auto,
        }))
        .map_err(|e| format!("publish started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::SummaryChunk {
            source: CompactionSource::Auto,
            delta: "a".to_string(),
            aggregated: "a".to_string(),
        }))
        .map_err(|e| format!("publish chunk a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::SummaryChunk {
            source: CompactionSource::Auto,
            delta: "b".to_string(),
            aggregated: "ab".to_string(),
        }))
        .map_err(|e| format!("publish chunk ab: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Completed {
            source: CompactionSource::Auto,
            summary: "ab".to_string(),
        }))
        .map_err(|e| format!("publish completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 3,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut sources: Vec<CompactionSource> = Vec::new();
    let mut aggregates: Vec<String> = Vec::new();
    let mut summary: Option<String> = None;
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Compaction(record) => match record {
                    CompactionRecord::Requested { source } => {
                        tags.push("compaction_requested");
                        sources.push(*source);
                    }
                    CompactionRecord::Started { source } => {
                        tags.push("compaction_started");
                        sources.push(*source);
                    }
                    CompactionRecord::SummaryChunk {
                        source, aggregated, ..
                    } => {
                        tags.push("compaction_chunk");
                        sources.push(*source);
                        aggregates.push(aggregated.clone());
                    }
                    CompactionRecord::Completed { summary: text, .. } => {
                        tags.push("compaction_completed");
                        summary = Some(text.clone());
                    }
                    CompactionRecord::Failed { .. } => tags.push("compaction_failed"),
                    CompactionRecord::Uncompacted { source } => {
                        tags.push("compaction_uncompacted");
                        sources.push(*source);
                    }
                },
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Tool(_) => tags.push("tool"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Prompt(_) => tags.push("prompt"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Session(_) => tags.push("session"),
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
            "compaction_requested",
            "compaction_started",
            "compaction_chunk",
            "compaction_chunk",
            "compaction_completed",
            "turn"
        ],
        "a compaction brackets its summary stream and the turn still closes"
    );
    assert!(
        sources
            .iter()
            .all(|source| *source == CompactionSource::Auto),
        "the compaction source is an enumerated producer fact"
    );
    for window in aggregates.windows(2) {
        assert!(
            window[1].len() >= window[0].len(),
            "aggregated must accumulate monotonically"
        );
    }
    assert_eq!(
        summary.as_deref(),
        Some("ab"),
        "the completed record carries the summary content"
    );
    Ok(())
}

/// The undo row is terminal on its own: it opens no bracket, and its source is
/// the enumerated producer fact, never text.
#[tokio::test]
async fn test_records_uncompacted_is_a_terminal_row() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Uncompacted {
            source: CompactionSource::Manual,
        }))
        .map_err(|e| format!("publish uncompacted: {e:?}"))?;
    drop(bus);

    // -- Check
    let record = rx.recv().await?;
    let SessionRecord::Compaction(CompactionRecord::Uncompacted { source }) = record else {
        panic!("the undo row must be Uncompacted, got: {record:?}");
    };
    assert_eq!(source, CompactionSource::Manual);
    assert!(
        matches!(rx.recv().await, Err(crate::bus::ReceiveError::Closed)),
        "the undo row is the whole sequence: nothing follows it"
    );
    Ok(())
}

/// A failed sweep narrates `Failed` instead of `Completed` and the region stays
/// untouched — the failure message is content.
#[tokio::test]
async fn test_records_compaction_failure_narrates_failed_not_completed() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Requested {
            source: CompactionSource::Manual,
        }))
        .map_err(|e| format!("publish requested: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Started {
            source: CompactionSource::Manual,
        }))
        .map_err(|e| format!("publish started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Compaction(CompactionRecord::Failed {
            source: CompactionSource::Manual,
            message: "summary model unavailable".to_string(),
        }))
        .map_err(|e| format!("publish failed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 4,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Compaction(CompactionRecord::Requested { .. }) => {
                    tags.push("compaction_requested")
                }
                SessionRecord::Compaction(CompactionRecord::Started { .. }) => {
                    tags.push("compaction_started")
                }
                SessionRecord::Compaction(CompactionRecord::Failed { message, .. }) => {
                    tags.push("compaction_failed");
                    failures.push(message.clone());
                }
                SessionRecord::Compaction(_) => tags.push("compaction_other"),
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Tool(_) => tags.push("tool"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Prompt(_) => tags.push("prompt"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Session(_) => tags.push("session"),
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
            "compaction_requested",
            "compaction_started",
            "compaction_failed",
            "turn"
        ],
        "a failed sweep narrates failure, never completion"
    );
    assert_eq!(
        failures,
        ["summary model unavailable"],
        "the failure message is content"
    );
    Ok(())
}

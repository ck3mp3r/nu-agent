//! Narration-contract tests: order-critical with an oldest-droppable tail.

use crate::bus::{Bus, NarrationEvent, ReceiveError};

use crate::test_support::Result;

/// A lagged narration consumer receives `Lagged`, re-syncs at the tail, and
/// from there stays in receipt order. The skipped records are the oldest ones;
/// the surviving tail is contiguous and increasing.
#[tokio::test]
async fn test_narration_lagged_consumer_keeps_receipt_order() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.narration().subscribe();
    const FLOOD: usize = 4_000;

    // -- Exec
    for i in 0..FLOOD {
        let _ = bus.narration().publish(NarrationEvent::InferenceChunk {
            delta: i.to_string(),
            aggregated: i.to_string(),
        });
    }
    drop(bus);

    // -- Check
    let mut lagged_at_least_once = false;
    let mut seen: Vec<usize> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(NarrationEvent::InferenceChunk { aggregated, .. }) => seen.push(aggregated.parse()?),
            Ok(other) => panic!("unexpected narration record: {other:?}"),
            Err(ReceiveError::Lagged(_)) => {
                lagged_at_least_once = true;
                continue;
            }
            Err(ReceiveError::Closed) => break,
        }
    }
    assert!(
        lagged_at_least_once,
        "a {FLOOD}-record flood must outrun the bounded narration ring"
    );
    assert!(
        !seen.is_empty(),
        "the lagged consumer must re-sync and receive the tail"
    );
    assert_eq!(
        seen.last().copied(),
        Some(FLOOD - 1),
        "the newest narration record must survive the lag"
    );
    for pair in seen.windows(2) {
        assert!(
            pair[0] < pair[1],
            "narration must stay in receipt order after the skip: {} then {}",
            pair[0],
            pair[1]
        );
    }
    let span = seen.last().copied().ok_or("the last narration index")?
        - seen.first().copied().ok_or("the first narration index")?
        + 1;
    assert_eq!(
        span,
        seen.len(),
        "the surviving tail must be contiguous — a skip drops a prefix, never a hole"
    );
    Ok(())
}

/// Under capacity, narration arrives complete and in order.
#[tokio::test]
async fn test_narration_under_capacity_arrives_complete() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.narration().subscribe();

    // -- Exec
    bus.narration()
        .publish(NarrationEvent::InferenceStarted)
        .map_err(|e| format!("narration should accept with a subscriber: {e:?}"))?;
    bus.narration()
        .publish(NarrationEvent::InferenceStopped {
            reason: "done".to_string(),
        })
        .map_err(|e| format!("narration should accept: {e:?}"))?;
    drop(bus);

    // -- Check
    let first = rx.recv().await.map_err(|e| format!("first: {e:?}"))?;
    assert!(
        matches!(first, NarrationEvent::InferenceStarted),
        "the first record should be InferenceStarted"
    );
    let second = rx.recv().await.map_err(|e| format!("second: {e:?}"))?;
    assert!(
        matches!(&second, NarrationEvent::InferenceStopped { reason } if reason == "done"),
        "the second record should be InferenceStopped with its reason"
    );
    Ok(())
}

//! Producer narration: the hook chain publishes session records on the kernel
//! stream in addition to the events it still sends on the hub.
//!
//! These tests drive a real rig turn through the real `HookChain` and read what
//! the producer published, so they pin the producer rather than the vocabulary.

use super::*;

use agnt_kernel::bus::Subscription;
use agnt_kernel::{InferenceRecord, SessionRecord};

#[tokio::test]
async fn test_narration_inference_lifecycle_brackets_the_completion() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("hello ".to_string()),
        MockStreamEvent::Text("world".to_string()),
        MockStreamEvent::final_response_with_total_tokens(42),
    ]]);
    let mut published = fixture.bus.stream().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let inference = inference_records(&mut published);
    let started = inference
        .first()
        .ok_or("the turn must narrate inference records")?;
    if !matches!(started, InferenceRecord::Started) {
        return Err(format!("the first record must be Started, got {started:?}").into());
    }
    let completed = inference
        .last()
        .ok_or("the turn must narrate an inference record")?;
    let InferenceRecord::Completed { usage } = completed else {
        return Err(format!("the last record must be Completed, got {completed:?}").into());
    };
    if usage.total_tokens != Some(42) {
        return Err(format!("the completion must carry the wire's total, got {usage:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn test_narration_chunks_accumulate_from_the_call_start() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("hello ".to_string()),
        MockStreamEvent::Text("world".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let mut published = fixture.bus.stream().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let chunks = chunk_pairs(&mut published);
    let expected: Vec<(String, String)> = vec![
        ("hello ".to_string(), "hello ".to_string()),
        ("world".to_string(), "hello world".to_string()),
    ];
    if chunks != expected {
        return Err(format!("chunks must accumulate from the call start, got {chunks:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn test_narration_chunks_match_the_events_the_hub_receives() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("alpha ".to_string()),
        MockStreamEvent::Text("beta".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let mut published = fixture.bus.stream().subscribe();
    let mut events = fixture.bus.ui_event().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let from_stream = chunk_pairs(&mut published);
    let from_hub = inference_chunks(&mut events);
    if from_stream != from_hub {
        return Err(format!(
            "the stream and the hub must narrate the same deltas, stream: {from_stream:?}, hub: {from_hub:?}"
        )
        .into());
    }
    Ok(())
}

/// The inference records the subscriber buffered, in publish order.
fn inference_records(rx: &mut Subscription<SessionRecord>) -> Vec<InferenceRecord> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Inference(inference) = record {
            out.push(inference);
        }
    }
    out
}

/// The `(delta, aggregated)` pairs the subscriber buffered, in publish order.
fn chunk_pairs(rx: &mut Subscription<SessionRecord>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Inference(InferenceRecord::Chunk {
            delta, aggregated, ..
        }) = record
        {
            out.push((delta, aggregated));
        }
    }
    out
}

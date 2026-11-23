//! The turn's stream: every call's deltas stream on the inference records, and no
//! call's deltas are folded as a compaction summary.

use super::super::*;

use agnt_kernel::{CompactionRecord, InferenceRecord, SessionRecord};

/// WHEN a call streams deltas, THE stream_delta branch SHALL publish the
/// InferenceChunk pair.
#[tokio::test]
async fn turn_deltas_stream_as_inference_chunks() -> Result<()> {
    // -- Setup & Fixtures: one ordinary turn, nothing in flight as a summary.
    let fx = RepetitionTurnFixture::new()?;
    fx.queue_turn(vec![identical_text_turn(1)]);
    let mut rx = fx.bus.stream().subscribe();
    let mut text_rx = fx.bus.stream().subscribe_text();

    // -- Exec
    let _ = fx.turn(false).await?;

    // -- Check
    let mut summary_chunks = 0;
    let mut inference_chunks = 0;
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. }) = record {
            summary_chunks += 1;
        }
    }
    while let Ok(record) = text_rx.try_recv() {
        if let SessionRecord::Inference(InferenceRecord::Chunk { .. }) = record {
            inference_chunks += 1;
        }
    }
    assert_eq!(inference_chunks, 1, "a normal call streams its own deltas");
    assert_eq!(
        summary_chunks, 0,
        "no call's deltas are a compaction summary in this turn"
    );
    Ok(())
}

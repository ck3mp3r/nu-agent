//! The patched call's stream: the summary this turn asked for streams on the
//! compaction records, not as an inference the transcript would show.

use super::super::*;

use agnt_kernel::{CompactionRecord, CompactionSource, InferenceRecord, SessionRecord};
use rig_memory::{SummaryCut, SummaryState};

/// WHEN the patched call streams deltas, THE stream_delta branch SHALL publish
/// `CompactionRecord::SummaryChunk` rows instead of the InferenceChunk pair.
#[tokio::test]
async fn summary_deltas_stream_as_compaction_chunks() -> Result<()> {
    // -- Setup & Fixtures: a turn whose request the decision patched, so the
    // deltas in flight belong to the summary.
    let fx = RepetitionTurnFixture::new()?;
    let pending = fx.pending_summary();
    pending.stash(
        SummaryCut {
            keep_from: None,
            state: SummaryState::default(),
        },
        CompactionSource::Auto,
        vec![],
    );
    fx.queue_turn(vec![identical_text_turn(1)]);
    let mut rx = fx.bus.stream().subscribe();

    // -- Exec
    let _ = fx.turn(false).await?;

    // -- Check: the deltas landed as compaction chunks, never as inference ones.
    let mut summary_chunks = 0;
    let mut inference_chunks = 0;
    while let Ok(record) = rx.try_recv() {
        match record {
            SessionRecord::Compaction(CompactionRecord::SummaryChunk { source, .. }) => {
                assert_eq!(source, CompactionSource::Auto);
                summary_chunks += 1;
            }
            SessionRecord::Inference(InferenceRecord::Chunk { .. }) => inference_chunks += 1,
            _ => {}
        }
    }
    assert_eq!(
        summary_chunks, 1,
        "the summary's delta must stream on the compaction records"
    );
    assert_eq!(
        inference_chunks, 0,
        "a patched call's deltas must not read back as inference chunks"
    );
    Ok(())
}

/// WHEN a call the decision did not patch streams deltas, THE stream_delta
/// branch SHALL CONTINUE TO publish the InferenceChunk pair.
#[tokio::test]
async fn unpatched_deltas_stream_as_inference_chunks() -> Result<()> {
    // -- Setup & Fixtures: no plan stashed, so nothing is in flight as a summary.
    let fx = RepetitionTurnFixture::new()?;
    fx.queue_turn(vec![identical_text_turn(1)]);
    let mut rx = fx.bus.stream().subscribe();

    // -- Exec
    let _ = fx.turn(false).await?;

    // -- Check
    let mut summary_chunks = 0;
    let mut inference_chunks = 0;
    while let Ok(record) = rx.try_recv() {
        match record {
            SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. }) => summary_chunks += 1,
            SessionRecord::Inference(InferenceRecord::Chunk { .. }) => inference_chunks += 1,
            _ => {}
        }
    }
    assert_eq!(inference_chunks, 1, "a normal call streams its own deltas");
    assert_eq!(summary_chunks, 0, "no summary was asked for in this turn");
    Ok(())
}

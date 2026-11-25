//! The pass identity each streamed summary chunk carries.
//!
//! A split turn runs two passes into one block: the history summary and the
//! split turn's earlier part. Every chunk says which pass produced it, so the
//! drawer paints one pass and drops the other without reading the text.

use super::*;

use agnt_kernel::SummaryStage;
use rig_memory::{CompactionPreparation, FileLists};

/// A preparation whose cut splits a turn: history to summarize and the split
/// turn's earlier part, both non-empty.
fn split_preparation() -> CompactionPreparation {
    CompactionPreparation {
        replaces: vec![Arc::new(Message::user("replaced question"))],
        to_summarize: vec![
            Arc::new(Message::user("first question")),
            Arc::new(Message::assistant("first answer")),
        ],
        turn_prefix: vec![Arc::new(Message::user("the split turn's question"))],
        previous: None,
        files: FileLists::default(),
    }
}

/// A whole-history preparation: nothing of the newest turn is cut off.
fn whole_history_preparation() -> CompactionPreparation {
    CompactionPreparation {
        replaces: vec![Arc::new(Message::user("replaced question"))],
        to_summarize: vec![
            Arc::new(Message::user("first question")),
            Arc::new(Message::assistant("first answer")),
        ],
        turn_prefix: Vec::new(),
        previous: None,
        files: FileLists::default(),
    }
}

/// A model whose first turn streams the history summary and whose second
/// streams the split turn's earlier part, two deltas each.
fn split_model() -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("history ".to_string()),
            MockStreamEvent::Text("summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("prefix ".to_string()),
            MockStreamEvent::Text("summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
}

/// The stages of the `SummaryChunk` records the text lane holds, in receipt
/// order.
fn drained_stages(rx: &mut Subscription<SessionRecord>) -> Vec<SummaryStage> {
    let mut stages = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Compaction(CompactionRecord::SummaryChunk { stage, .. }) = record {
            stages.push(stage);
        }
    }
    stages
}

#[tokio::test]
async fn summarize_stamps_a_split_turns_chunks_with_their_pass() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut text_rx = bus.stream().subscribe_text();
    let model = split_model().erase();
    let spec = test_spec()?;
    let compactor = NuCompactor::new(model.clone(), bus, test_settings(), Some(spec.clone()));

    // -- Exec
    let summary = compactor
        .summarize(&model, &split_preparation(), &spec, CompactionSource::Auto)
        .await?;

    // -- Check: every history delta reads History, every prefix delta reads
    // TurnPrefix.
    assert_eq!(
        drained_stages(&mut text_rx),
        vec![
            SummaryStage::History,
            SummaryStage::History,
            SummaryStage::TurnPrefix,
            SummaryStage::TurnPrefix,
        ],
        "each pass's deltas carry that pass's stage"
    );

    // -- Check: both passes still reach the summary the completion carries.
    assert!(
        summary.contains("history summary"),
        "the history pass reaches the final summary, got: {summary}"
    );
    assert!(
        summary.contains("prefix summary"),
        "the split turn's part reaches the final summary, got: {summary}"
    );
    Ok(())
}

#[tokio::test]
async fn summarize_stamps_a_whole_history_with_the_history_stage() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut text_rx = bus.stream().subscribe_text();
    let model = stream_model(&["first ", "second"]).erase();
    let spec = test_spec()?;
    let compactor = NuCompactor::new(model.clone(), bus, test_settings(), Some(spec.clone()));

    // -- Exec
    let summary = compactor
        .summarize(
            &model,
            &whole_history_preparation(),
            &spec,
            CompactionSource::Auto,
        )
        .await?;

    // -- Check: a compaction that splits no turn streams one pass, the history
    // one, exactly as it always has.
    assert_eq!(
        drained_stages(&mut text_rx),
        vec![SummaryStage::History, SummaryStage::History],
        "a whole-history compaction streams the history pass alone"
    );
    assert_eq!(summary, "first second");
    Ok(())
}

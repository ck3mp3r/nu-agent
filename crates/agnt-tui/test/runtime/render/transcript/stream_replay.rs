use super::*;

use crate::interaction::reducer::{RenderInput, reduce};
use agnt_runtime::protocol::events::NarrationEvent;

/// Pane width the replay paints at.
const COLUMNS: u16 = 44;
/// Frame height the replay paints at: shorter than the final answer, so the
/// window and the scroll math are part of what is compared.
const ROWS: u16 = 14;
/// Chunks the many-chunk replay folds.
const CHUNKS: usize = 24;

/// WHEN the same stream is replayed as many chunks with a paint between each,
/// THE pane SHALL paint byte-identical rows and plan identical geometry to the
/// one-chunk render of the same final text.
///
/// The pre-change fold rebuilt the whole entry and re-measured it per chunk;
/// the in-place fold must land on exactly the same picture, so this is the
/// golden replay the fold is checked against.
#[test]
fn test_transcript_stream_replay_paints_identically_to_one_chunk() -> Result<()> {
    // -- Setup & Fixtures: two live sessions with the same starting transcript.
    let mut streamed = RuntimeCoordinator::new(COLUMNS, ROWS, None);
    let mut once = RuntimeCoordinator::new(COLUMNS, ROWS, None);

    // -- Exec: the streamed session folds every chunk and paints after each.
    for chunk in 1..=CHUNKS {
        fold_chunk(&mut streamed, &accumulated_text(chunk));
        let _ = render_transcript_pane_to_backend(&mut streamed, COLUMNS, ROWS, true, 0);
    }
    let streamed_frame = render_transcript_pane_to_backend(&mut streamed, COLUMNS, ROWS, true, 0);

    // -- Exec: the single-chunk session folds the final text once and paints.
    fold_chunk(&mut once, &accumulated_text(CHUNKS));
    let once_frame = render_transcript_pane_to_backend(&mut once, COLUMNS, ROWS, true, 0);

    // -- Check: byte-identical rows, identical geometry.
    assert_eq!(
        painted_rows(&streamed_frame),
        painted_rows(&once_frame),
        "a streamed replay must paint the same rows as one render of the same text"
    );
    let width = usize::from(COLUMNS - 2);
    let len = streamed.state.transcript.len();
    assert_eq!(
        streamed.state.transcript.frame_plan(0, len, width),
        once.state.transcript.frame_plan(0, len, width),
        "the streamed session must plan the same geometry as the one-chunk render"
    );
    assert_eq!(
        streamed.state.transcript.total_visual_rows(),
        once.state.transcript.total_visual_rows(),
        "the streamed session must measure the same total rows"
    );
    Ok(())
}

// region:    --- Test Support

/// Fold one inference chunk carrying `aggregated` through the reducer.
fn fold_chunk(coord: &mut RuntimeCoordinator, aggregated: &str) {
    let event = NarrationEvent::InferenceChunk {
        inference: crate::test_support::inference(),
        delta: aggregated.to_string(),
        aggregated: aggregated.to_string(),
    };
    let changed = reduce(&mut coord.state, RenderInput::Narration(&event), None);
    assert!(changed, "a non-empty chunk must report a change");
}

/// The accumulated answer after `chunk` chunks: prose with a heading and a
/// list, so the markdown projection is part of the comparison.
fn accumulated_text(chunk: usize) -> String {
    let mut text = String::from("## Plan\n");
    for line in 0..chunk {
        text.push_str(&format!(
            "\n- step {line} rewraps once at this pane width\n"
        ));
    }
    text
}

/// Every painted row of the terminal buffer, trailing blanks trimmed.
fn painted_rows(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            row.trim_end().to_string()
        })
        .collect()
}

// endregion: --- Test Support

use super::*;

use crate::state::{StatusMessageKind, TranscriptStore};
use agnt_render::transcript::entry::{ToolEntry, ToolKind};
use agnt_render::transcript::ir::{ContentKind, Display, DisplaySection, ToolName};
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::events::ToolCall;

/// The width the store indexes at in these tests. Wide enough that a prose line
/// wraps once.
const WIDTH: usize = 48;

/// Chunks folded between two paints: the store must re-measure the streamed
/// entry once for the whole burst, not once per chunk.
const CHUNKS: usize = 40;

// ---------------------------------------------------------------------------
// A burst of chunks costs one measure per frame
// ---------------------------------------------------------------------------

/// WHEN 40 chunks fold between two paints, THE store SHALL re-measure the
/// streamed entry at most once for those folds plus once at the paint — the
/// entry keeps its identity throughout and the paint's rows match a wholesale
/// rebuild.
#[test]
fn test_llm_assistant_stream_folds_chunks_within_one_frame_measure() -> Result<()> {
    // -- Setup & Fixtures: a streamed turn with an indexed transcript.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("first chunk"));
    state.transcript.rebuild_height_index(WIDTH);
    state.transcript.ensure_height_index(WIDTH);
    let start = state
        .transcript
        .assistant_stream_start
        .ok_or("the stream must have a block")?;
    let measured_before = state.transcript.remeasured_total();

    // -- Exec: fold a burst of chunks, each replacing the accumulated text.
    for chunk in 1..=CHUNKS {
        reduce_llm(&mut state, assistant_message(&accumulated_text(chunk)));
    }

    // -- Check: no measure during the folds, and the entry never moved.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a chunk fold must not measure the streamed entry"
    );
    assert_eq!(
        state.transcript.assistant_stream_start,
        Some(start),
        "the streamed entry must keep its slot across chunks"
    );
    assert_eq!(
        state.transcript.len(),
        start + 1,
        "the streamed block is one entry, never one entry per chunk"
    );

    // -- Exec: the paint reads the index.
    state.transcript.ensure_height_index(WIDTH);

    // -- Check: one measure for the burst, and the rows match a wholesale
    // rebuild of the accumulated text.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "one frame measures the streamed entry once"
    );
    assert_eq!(
        assistant_markdown_entries(&state),
        vec![accumulated_text(CHUNKS)],
        "the streamed entry carries the last accumulated text"
    );
    assert_geometry_matches_wholesale(&state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The diff-gate projection runs once per paint window
// ---------------------------------------------------------------------------

/// WHEN the folded text carries a diff signature, THE diff gate SHALL compute
/// its projection at most once per paint window: the answer found in one window
/// holds for every later chunk of that window, and the next painted frame asks
/// again.
#[test]
fn test_llm_assistant_stream_diff_gate_answers_once_per_frame() -> Result<()> {
    // -- Setup & Fixtures: a completed edit showing a diff for another file, so
    // the sample diff is not redundant, and a fresh paint window.
    let mut state = AppState::default();
    push_edit_with_diff(&mut state, "other");
    state.transcript.rebuild_height_index(WIDTH);
    state.transcript.ensure_height_index(WIDTH);

    // -- Exec: the first sample-diff chunk of the window.
    reduce_llm(&mut state, assistant_message(SAMPLE_DIFF));

    // -- Check: it is not redundant, so it is stored.
    assert_eq!(
        state.transcript.len(),
        2,
        "a diff the tool does not show must be stored"
    );

    // -- Setup & Fixtures: the tool display now carries the same diff.
    state
        .transcript
        .set_tool_result_at(0, diff_display(&["sample"]));

    // -- Exec: a second chunk of the same window.
    reduce_llm(&mut state, assistant_message(SAMPLE_DIFF));

    // -- Check: the window's answer is reused, so no projection re-ran and the
    // entry survives.
    assert_eq!(
        state.transcript.len(),
        2,
        "one window computes the gate's projection once"
    );

    // -- Exec: the next frame opens a new window, then the chunk folds again.
    state.transcript.ensure_height_index(WIDTH);
    reduce_llm(&mut state, assistant_message(SAMPLE_DIFF));

    // -- Check: the new window asked again and dropped the regurgitation.
    assert_eq!(
        state.transcript.len(),
        1,
        "the next window must re-run the projection and suppress the diff"
    );
    assert!(!state.transcript.entries().iter().any(is_assistant_entry));
    Ok(())
}

// ---------------------------------------------------------------------------
// The gate's payment is recorded once per paint window
// ---------------------------------------------------------------------------

/// WHEN the fold pays the diff gate's projection, THE store's `diff_gate_cost`
/// calls SHALL grow by exactly one; WHEN a second chunk of the same window
/// folds, THE calls SHALL NOT grow.
#[test]
fn test_llm_assistant_stream_diff_gate_records_one_projection_per_window() -> Result<()> {
    // -- Setup & Fixtures: a completed edit showing a diff for another file, so
    // the sample diff is not redundant, and a fresh paint window.
    let mut state = AppState::default();
    push_edit_with_diff(&mut state, "other");
    state.transcript.rebuild_height_index(WIDTH);
    state.transcript.ensure_height_index(WIDTH);
    let calls_before = state.transcript.diff_gate_cost().1;

    // -- Exec: the first diff-signature chunk of the window.
    reduce_llm(&mut state, assistant_message(SAMPLE_DIFF));

    // -- Check: the fold's own call recorded the projection.
    assert_eq!(
        state
            .transcript
            .diff_gate_cost()
            .1
            .saturating_sub(calls_before),
        1,
        "the first diff-signature chunk must record one projection"
    );

    // -- Exec: a second chunk of the same window.
    reduce_llm(&mut state, assistant_message(SAMPLE_DIFF));

    // -- Check: the window's answer holds, so no projection was re-paid.
    assert_eq!(
        state
            .transcript
            .diff_gate_cost()
            .1
            .saturating_sub(calls_before),
        1,
        "a later chunk of the same window must record nothing"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The silent-discard path keeps working and leaves no pending measure
// ---------------------------------------------------------------------------

/// WHEN a retried turn's empty-reason stop folds, THE provisional block SHALL
/// still be discarded — and the measure it had pending SHALL go with it, so the
/// next paint neither measures a dropped entry nor keeps a stale index.
#[test]
fn test_llm_assistant_stream_silent_discard_drops_the_pending_measure() -> Result<()> {
    // -- Setup & Fixtures: a streamed block with a fold pending.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("provisional one"));
    state.transcript.rebuild_height_index(WIDTH);
    state.transcript.ensure_height_index(WIDTH);
    for chunk in 2..=5 {
        reduce_llm(&mut state, assistant_message(&accumulated_text(chunk)));
    }
    let measured_before = state.transcript.remeasured_total();
    assert!(
        state.transcript.entries().iter().any(is_assistant_entry),
        "the provisional block must exist before the stop"
    );

    // -- Exec: the retried turn's silent discard.
    let changed = reduce_llm(&mut state, inference_stopped(String::new()));

    // -- Check: the block is gone and the cursor is closed.
    assert!(!changed, "a silent discard renders nothing");
    assert!(!state.transcript.entries().iter().any(is_assistant_entry));
    assert!(state.transcript.assistant_stream_start.is_none());

    // -- Exec: the next paint reads the index.
    state.transcript.ensure_height_index(WIDTH);

    // -- Check: nothing was measured for the dropped entry, and the index
    // matches a wholesale rebuild of what is left.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a dropped entry must leave no pending measure"
    );
    assert_geometry_matches_wholesale(&state)?;
    Ok(())
}

/// A `UserCancelled` stop keeps the streamed block and closes it: the next
/// call's chunk starts a fresh entry rather than folding into the closed one.
#[test]
fn test_llm_assistant_stream_closed_block_starts_a_new_entry() -> Result<()> {
    // -- Setup & Fixtures: a streamed block, kept by a user-cancel stop.
    let mut state = busy_state_with_clean_transcript();
    reduce_llm(&mut state, assistant_message("partial answer"));
    let first_start = state
        .transcript
        .assistant_stream_start
        .ok_or("the stream must have a block")?;
    reduce_llm(
        &mut state,
        inference_stopped_with_cause(
            agnt_runtime::protocol::events::StopCause::UserCancelled,
            "Cancelled by user".to_string(),
        ),
    );
    assert_eq!(
        state.status.message.kind(),
        StatusMessageKind::Notice,
        "the cancel notice stays in the status bar"
    );

    // -- Exec: the next call streams.
    reduce_llm(&mut state, inference_started());
    reduce_llm(&mut state, assistant_message("second answer"));

    // -- Check: the first block is untouched and the new text is its own entry.
    assert_eq!(
        state.transcript.assistant_stream_start,
        Some(first_start + 1),
        "a closed block must not be folded into"
    );
    assert_eq!(
        assistant_markdown_entries(&state),
        vec!["partial answer".to_string(), "second answer".to_string()],
        "each call keeps its own block"
    );
    Ok(())
}

// -- Test Support

/// The sample diff the fixture edit ends up showing.
const SAMPLE_DIFF: &str =
    "```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```";

/// The accumulated answer after `chunk` chunks: `chunk` prose lines.
fn accumulated_text(chunk: usize) -> String {
    (0..chunk)
        .map(|line| format!("line {line} of the streamed answer carries enough words to wrap"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Push a completed edit whose display carries a diff for `name`.
fn push_edit_with_diff(state: &mut AppState, name: &str) {
    state
        .transcript
        .push_entry(TranscriptEntry::Tool(ToolEntry {
            kind: ToolKind::Edit,
            call: ToolCall::new(
                ToolName("edit".to_string()),
                agnt_runtime::tools::handler::ToolSource::Closure,
                "{\"path\":\"sample.txt\"}",
            ),
            preview: None,
            result: Some(diff_display(&[name])),
            status: ItemStatus::Done,
        }));
}

/// One unified-diff display for the named files.
fn diff_display(names: &[&str]) -> Display {
    Display {
        title: Some("edit".to_string()),
        sections: names
            .iter()
            .map(|name| DisplaySection {
                label: Some(format!("{name}.txt")),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: format!("{name}.txt"),
                },
                content: format!("--- a/{name}.txt\n+++ b/{name}.txt\n@@ -1 +1 @@\n-old\n+new\n"),
                stats: None,
            })
            .collect(),
    }
}

/// Assert the store's cached geometry equals a wholesale rebuild of its own
/// entries at the test width.
fn assert_geometry_matches_wholesale(state: &AppState) -> Result<()> {
    let mut wholesale: TranscriptStore = state.transcript.clone();
    wholesale.rebuild_height_index(WIDTH);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale.total_visual_rows(),
        "the deferred measure must match a wholesale rebuild"
    );
    let len = state.transcript.len();
    assert_eq!(
        state.transcript.frame_plan(0, len, WIDTH),
        wholesale.frame_plan(0, len, WIDTH),
        "the frame plan must match a wholesale rebuild"
    );
    Ok(())
}

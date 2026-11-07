use super::*;

use crate::interaction::reducer::reduce;
use crate::widgets::RatatuiBackend;
use agnt_core::protocol::events::{SystemEvent, ToolOutcome};
use agnt_render::RenderBackend;
use agnt_runtime::session::tool_status::ToolStatus;

/// The pane width every fixture renders at. The list area is two columns
/// narrower: one margin column on each side.
const COLUMNS: u16 = 40;
/// Frame height every fixture renders at: taller than the fixture transcript.
const ROWS: u16 = 12;
/// First completion-display body line. Distinctive, so a count names it.
const ALPHA: &str = "alpha-display-line";
/// Second completion-display body line.
const BETA: &str = "beta-display-line";

// ---------------------------------------------------------------------------
// One completion display, painted once (regression: double render)
// ---------------------------------------------------------------------------

/// WHEN a tool call completes with a display and the height index rebuilds
/// (a resize), THE pane SHALL paint every completion-display line exactly once.
///
/// The display lives on ONE entry slot: the completion writes `ToolEntry::result`
/// in place, and every backend reads `result.or(preview)`. A stale cached metric
/// or a companion display entry would double the rows.
#[test]
fn test_transcript_display_completion_paints_once_after_a_rebuild() -> Result<()> {
    // -- Setup & Fixtures: a live session. The pane paints before the call
    // starts, so the height index caches the rows of every pushed entry.
    let mut coord = RuntimeCoordinator::new(COLUMNS, ROWS, None);
    let before_call = render_transcript_pane_to_backend(&mut coord, COLUMNS, ROWS, true, 0);
    assert_eq!(
        occurrences(&before_call, ALPHA),
        0,
        "the fixture transcript must start without the display"
    );
    start_call(&mut coord);
    complete_call(&mut coord);

    // -- Exec: paint, then force a height-index rebuild with a resize.
    let before_rebuild = render_transcript_pane_to_backend(&mut coord, COLUMNS, ROWS, true, 0);
    let after_rebuild = render_transcript_pane_to_backend(&mut coord, COLUMNS + 10, ROWS, true, 0);

    // -- Check: each display line paints exactly once, before and after the
    // rebuild the resize triggers.
    for (terminal, when) in [
        (&before_rebuild, "before the rebuild"),
        (&after_rebuild, "after the resize rebuild"),
    ] {
        for needle in [ALPHA, BETA] {
            assert_eq!(
                occurrences(terminal, needle),
                1,
                "{needle:?} must paint exactly once {when}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The in-place completion mutation re-measures the cached rows
// ---------------------------------------------------------------------------

/// WHEN a tool call completes with a display and the pane repaints without a
/// resize, THE height index SHALL re-measure the mutated entry in place, so
/// every planned entry spans exactly its measured rows — no stale metric clips
/// the body and no wholesale rebuild runs.
#[test]
fn test_transcript_display_completion_remeasures_the_height_index() -> Result<()> {
    // -- Setup & Fixtures: a live session with the pane already painted, so a
    // cached metric exists for the entry the completion mutates.
    let mut coord = RuntimeCoordinator::new(COLUMNS, ROWS, None);
    let width = usize::from(COLUMNS.saturating_sub(2));
    let _ = render_transcript_pane_to_backend(&mut coord, COLUMNS, ROWS, true, 0);
    start_call(&mut coord);

    // -- Exec: the completion records the display on the entry in place.
    complete_call(&mut coord);

    // -- Check: the in-place mutation re-measures the entry and leaves the
    // index valid — no wholesale rebuild is needed for the next paint.
    assert!(
        coord.state.transcript.height_index_valid_for(width),
        "the completion display must re-measure in place, not invalidate"
    );

    // -- Exec: the next paint plans the mutated entry — no resize, no rebuild.
    let _ = render_transcript_pane_to_backend(&mut coord, COLUMNS, ROWS, true, 0);

    // -- Check: every planned entry spans exactly its measured rows.
    let plan = coord
        .state
        .transcript
        .frame_plan(0, coord.state.transcript.len(), width);
    assert!(
        !plan.entries.is_empty(),
        "the fixture transcript is planned"
    );
    let probe = RatatuiBackend::new(0, 0);
    for planned in &plan.entries {
        let entry = coord
            .state
            .transcript
            .entries()
            .get(planned.index)
            .ok_or("a planned index must address an entry")?;
        assert_eq!(
            planned.rect.rows,
            probe.measure(entry, width),
            "entry {} must span its measured rows",
            planned.index
        );
    }
    Ok(())
}

// region:    --- Test Support

/// The fixture tool call: an edit of `a.rs`, started and completed with one key.
fn fixture_call() -> ToolCall {
    ToolCall::new(
        ToolName("edit".to_string()),
        agnt_runtime::tools::handler::ToolSource::Mcp,
        r#"{"path":"a.rs"}"#,
    )
}

/// Fold a `ToolStarted` event for the fixture call through the reducer.
fn start_call(coord: &mut RuntimeCoordinator) {
    let changed = reduce(
        &mut coord.state,
        SystemEvent::ToolStarted {
            call: fixture_call(),
        },
        None,
    );
    assert!(changed, "starting a tool call must report a change");
}

/// Fold a `ToolCompleted` event whose result carries the fixture diff through
/// the reducer. The surface derives the completion body from that data.
fn complete_call(coord: &mut RuntimeCoordinator) {
    let changed = reduce(
        &mut coord.state,
        SystemEvent::ToolCompleted {
            call: fixture_call(),
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: format!("{{\"path\":\"a.rs\",\"diff\":\"{ALPHA}\\n{BETA}\"}}"),
                error_kind: None,
                message: None,
            },
        },
        None,
    );
    assert!(changed, "completing a tool call must report a change");
}

/// How many times `needle` occurs across the painted buffer's rows.
fn occurrences(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, needle: &str) -> usize {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            row.matches(needle).count()
        })
        .sum()
}

// endregion: --- Test Support

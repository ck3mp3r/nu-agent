use crate::RenderBackend;
use crate::gallery::Gallery;
use crate::plain::PlainBackend;
use crate::planner::LayoutPlanner;
use crate::theme::Theme;
use agnt_core::transcript::entry::TranscriptEntry;

/// The parity property (design §4.2): the plan's row accounting equals the
/// backend's rendered row count over the gallery corpus, and every planned
/// rect is exactly what the backend measured.
#[test]
fn test_parity_plan_rows_equal_rendered_rows() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;
    let backend = PlainBackend::default();
    let entries: Vec<TranscriptEntry> = Gallery::samples()
        .into_iter()
        .map(|sample| sample.entry)
        .collect();

    // -- Exec
    let plan = LayoutPlanner::plan(&entries, width, &backend);
    let mut painting = PlainBackend::default();
    for planned in &plan.entries {
        let entry = entries.get(planned.index).ok_or("planned index in range")?;
        painting.render(entry, planned.rect, &Theme::default());
    }

    // -- Check
    assert_eq!(
        painting.line_count(),
        plan.total_rows,
        "painted rows must equal the planned total"
    );

    let mut accounted = plan.trailing_gap;
    for planned in &plan.entries {
        let entry = entries.get(planned.index).ok_or("planned index in range")?;
        assert_eq!(
            planned.rect.rows,
            backend.measure(entry, width) + owned_padding(entry, planned.index, entries.len()),
            "rect rows must equal the backend's measurement plus the entry's own padding"
        );
        accounted += planned.gap_before + planned.rect.rows;
    }
    assert_eq!(
        accounted, plan.total_rows,
        "gaps, content, and trailing gap add up"
    );
    Ok(())
}

/// Rendering the corpus through the plan leaves exactly the planned gap rows
/// blank: no row is silently dropped and no gap row carries content.
#[test]
fn test_parity_gap_rows_render_blank() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;
    let backend = PlainBackend::default();
    let entries: Vec<TranscriptEntry> = Gallery::samples()
        .into_iter()
        .map(|sample| sample.entry)
        .collect();

    // -- Exec
    let plan = LayoutPlanner::plan(&entries, width, &backend);
    let mut painting = PlainBackend::default();
    for planned in &plan.entries {
        let entry = entries.get(planned.index).ok_or("planned index in range")?;
        painting.render(entry, planned.rect, &Theme::default());
    }

    // -- Check
    for planned in &plan.entries {
        for row in planned.rect.row - planned.gap_before..planned.rect.row {
            let line = painting.output().get(row).ok_or("gap row is written")?;
            assert!(
                line.is_empty(),
                "gap row {row} must stay blank, got {line:?}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The blank rows the planner adds to an entry's rect: the entry's own padding,
/// clipped at the frame edges — the first entry keeps no top padding and the
/// last no bottom padding, so the frame's first and last rows are content rows.
fn owned_padding(entry: &TranscriptEntry, index: usize, len: usize) -> usize {
    let (top, bottom) = entry.padding();
    let top = if index == 0 { 0 } else { top };
    let bottom = if index + 1 == len { 0 } else { bottom };
    top + bottom
}

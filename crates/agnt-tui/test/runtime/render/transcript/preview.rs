use super::*;

use agnt_core::protocol::contracts::UiMessageSnapshot;
use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use agnt_core::session::tool_status::ToolStatus;
use agnt_render::RenderBackend;

use crate::widgets::RatatuiBackend;

// ---------------------------------------------------------------------------
// One tool entry with a code preview (task 5c99283f)
// ---------------------------------------------------------------------------

/// WHEN the store holds ONE tool entry carrying a code preview, THE widget SHALL
/// render the call line untinted and the preview on the code surface between two
/// margin rows — one entry, one call line, no companion entry.
#[test]
fn test_transcript_preview_tool_entry_renders_one_code_surface() -> Result<()> {
    // -- Setup & Fixtures: one tool entry carrying the code preview.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    coord
        .state
        .transcript
        .push_entry(nu_tool_entry("ls | select name"));
    assert_eq!(coord.state.transcript.len(), 1, "one entry, no companion");

    // -- Exec: a viewport exactly as tall as the entry, so the entry owns
    // every row and no padding row shifts it down.
    let width = columns.saturating_sub(2) as usize;
    let rows = u16::try_from(
        RatatuiBackend::new(0, 0).measure(
            coord
                .state
                .transcript
                .entries()
                .first()
                .ok_or("should have the pushed entry")?,
            width,
        ),
    )
    .map_err(|_| "rows must fit u16")?;
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: row 0 is the untinted call line; from the top margin below it
    // every row paints the code surface out to the list edge.
    let base = coord.theme.base;
    let surface = coord.theme.surface0;
    assert!(
        (0..columns)
            .map(|x| cell_symbol(&terminal, x, 0))
            .collect::<String>()
            .contains("ls | select name"),
        "the call line carries the summary"
    );
    assert_eq!(
        cell_bg(&terminal, 1, 0),
        Some(base),
        "the call line is untinted"
    );
    assert!(rows > 4, "the surface needs two margins plus its rows");
    for y in 1..rows {
        assert_eq!(
            cell_bg(&terminal, 1, y),
            Some(surface),
            "row {y} paints the code surface"
        );
        assert_eq!(
            cell_bg(&terminal, columns - 3, y),
            Some(surface),
            "row {y} paints the surface to the list edge"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Diff tint wins over the code surface (task 669b4dac)
// ---------------------------------------------------------------------------

/// WHEN a tool entry's preview carries a diff, THE added row SHALL paint the add
/// tint across the row — the diff tint SHALL win over the code surface.
#[test]
fn test_transcript_preview_diff_tint_wins_over_code_surface() -> Result<()> {
    // -- Setup & Fixtures: a tool entry with a diff preview.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    let rows = 8u16;
    let call = ToolCall::new(
        ToolName("edit".to_string()),
        "",
        "",
        CallLine {
            summary: "→ a/a.rs".to_string(),
        },
    );
    let entry = ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call,
        preview: Some(Display {
            title: None,
            sections: vec![DisplaySection {
                label: None,
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "a/a.rs".to_string(),
                },
                content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: ItemStatus::Done,
    };
    coord
        .state
        .transcript
        .push_entry(TranscriptEntry::Tool(entry));

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: the row carrying the added body paints the add tint, not the
    // code surface, and the tint covers the whole row.
    let add_bg = coord.theme.diff_add_bg;
    let surface = coord.theme.surface0;
    let added = (0..rows)
        .find(|y| {
            (0..columns)
                .map(|x| cell_symbol(&terminal, x, *y))
                .collect::<String>()
                .contains("new")
        })
        .ok_or("the rendered buffer must contain the added diff body")?;
    assert_eq!(
        cell_bg(&terminal, 1, added),
        Some(add_bg),
        "the added diff row must paint the add tint"
    );
    assert_eq!(
        cell_bg(&terminal, columns - 3, added),
        Some(add_bg),
        "the add tint covers the whole row"
    );
    assert_ne!(
        cell_bg(&terminal, 1, added),
        Some(surface),
        "the code surface must not clobber the diff tint"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Rehydrated tool rows keep their gate bands (task 72f47717)
// ---------------------------------------------------------------------------

/// WHEN a hydrated tool call carries a display, THE replayed row SHALL paint it
/// as the banded gate-preview block under the untinted call line with the
/// persisted status glyph — main's hydrated look for every outcome (✓ success,
/// ✕ failed, ⊘ cancelled). The band keys on the display sitting in the entry's
/// PREVIEW slot, which is where main's hydration docked it.
#[test]
fn test_transcript_preview_hydrated_rows_paint_the_gate_band_per_status() -> Result<()> {
    // -- Setup & Fixtures: one persisted tool pair per outcome.
    let cases: [(&str, ToolStatus, char); 3] = [
        ("cancelled", ToolStatus::Cancelled, '\u{2298}'),
        ("failed", ToolStatus::Failed, '\u{2715}'),
        ("success", ToolStatus::Success, '\u{2713}'),
    ];
    let columns = 40u16;

    for (label, status, glyph) in cases {
        let mut coord = RuntimeCoordinator::new(40, 8, None);
        let snapshot = UiMessageSnapshot::new("tool", "→ ⏱ 120s")
            .with_tool_name("nu".to_string())
            .with_tool_details(
                Some(r#"{"command":"ls | select name"}"#.to_string()),
                None,
                Some(matches!(status, ToolStatus::Success)),
            )
            .with_tool_status(status);

        // -- Exec: hydrate the pair, then render a viewport exactly as tall.
        coord.hydrate_transcript_from_messages(
            vec![
                snapshot,
                UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(
                    ToolDisplay {
                        title: None,
                        sections: vec![ToolDisplaySection {
                            label: None,
                            kind: ContentKind::Code {
                                language: "nu".to_string(),
                            },
                            content: "ls | select name".to_string(),
                            stats: None,
                        }],
                    },
                ),
            ],
            None,
        );
        assert_eq!(
            coord.state.transcript.len(),
            1,
            "{label}: one call, one entry"
        );
        let width = columns.saturating_sub(2) as usize;
        let rows = u16::try_from(
            RatatuiBackend::new(0, 0).measure(
                coord
                    .state
                    .transcript
                    .entries()
                    .first()
                    .ok_or("should have the hydrated entry")?,
                width,
            ),
        )
        .map_err(|_| "rows must fit u16")?;
        let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

        // -- Check: the status-glyph call line opens the entry untinted, and
        // every row below it paints the code surface out to the list edge.
        let base = coord.theme.base;
        let surface = coord.theme.surface0;
        let call_row: String = (0..columns).map(|x| cell_symbol(&terminal, x, 0)).collect();
        assert!(
            call_row.contains(glyph),
            "{label}: the call line carries {glyph:?}; got {call_row:?}"
        );
        assert_eq!(
            cell_bg(&terminal, 1, 0),
            Some(base),
            "{label}: the call line is untinted"
        );
        assert!(
            rows >= 4,
            "{label}: the surface needs two margins around its one body row"
        );
        let mut painted: String = String::new();
        for y in 1..rows {
            assert_eq!(
                cell_bg(&terminal, 1, y),
                Some(surface),
                "{label}: row {y} paints the code surface"
            );
            assert_eq!(
                cell_bg(&terminal, columns - 3, y),
                Some(surface),
                "{label}: row {y} paints the surface to the list edge"
            );
            painted.extend((0..columns).map(|x| cell_symbol(&terminal, x, y)));
        }
        assert!(
            painted.contains("ls | select name"),
            "{label}: the band carries the nu command; got {painted:?}"
        );
    }
    Ok(())
}

/// WHEN a code-preview tool entry is followed by another tool entry, THE plan
/// SHALL account exactly one row between them: the display's own closing pad
/// row, with no planner separator on top of it.
#[test]
fn test_transcript_preview_adjacent_tool_entries_carry_no_separator() -> Result<()> {
    // -- Setup & Fixtures: a nu call with its code preview, then a read call.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    coord
        .state
        .transcript
        .push_entry(nu_tool_entry("ls | select name"));
    coord
        .state
        .transcript
        .push_entry(tool_entry("read", "→ a.rs"));

    // -- Exec: build the accounting at the list width.
    let width = columns.saturating_sub(2) as usize;
    coord.state.transcript.rebuild_height_index(width);
    let probe = RatatuiBackend::new(0, 0);
    let measured: usize = coord
        .state
        .transcript
        .entries()
        .iter()
        .map(|entry| probe.measure(entry, width))
        .sum();

    // -- Check: the pair carries no planner separator; the surface-carrying tool
    // closes with its own blank row (main's spacing between a display and the
    // next call), and both call lines render.
    assert_eq!(
        coord.state.transcript.total_visual_rows(),
        measured + 1,
        "the display's closing pad row is the whole spacing"
    );
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, 8, true, 0);
    let text: String = (0..8)
        .flat_map(|y| (0..columns).map(move |x| (x, y)))
        .map(|(x, y)| cell_symbol(&terminal, x, y))
        .collect();
    assert!(text.contains("ls | select name"), "got:\n{text}");
    assert!(text.contains("→ a.rs"), "got:\n{text}");
    Ok(())
}

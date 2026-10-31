use super::*;

use agnt_render::backend::RowRect;
use agnt_render::planner::LayoutPlanner;
use agnt_render::{RenderBackend, Theme};

use crate::widgets::RatatuiBackend;

/// WHEN the store accounts a tool entry whose preview is a code surface, THE
/// accounting SHALL include the band's two margin rows plus every surface row,
/// and THE pane SHALL paint the top margin above an untinted call line and the
/// body below it.
#[test]
fn test_transcript_layout_tool_code_preview_accounts_surface_margins() -> Result<()> {
    // -- Setup & Fixtures: one call row and a three-line code preview.
    let entry = nu_tool_entry("alpha\nbeta\ngamma");
    let mut store = crate::state::TranscriptStore::default();
    store.push_entry(entry.clone());

    // -- Exec & Check
    for width in [80usize, 120] {
        store.rebuild_height_index(width);
        let rows = RatatuiBackend::new(0, 0).measure(&entry, width);
        assert_eq!(
            store.total_visual_rows(),
            rows,
            "width {width}: accounting must match the measured rows"
        );

        let columns = u16::try_from(width).map_err(|_| "width must fit u16")?;
        let height = u16::try_from(rows).map_err(|_| "rows must fit u16")?;
        let mut backend = RatatuiBackend::new(columns, height);
        backend.render(&entry, RowRect::new(0, width, rows), &Theme::default());
        let surface = Theme::default().code_surface;
        let surface_bg = ratatui::style::Color::Rgb(surface.r, surface.g, surface.b);

        assert!(rows > 4, "width {width}: the surface needs margins + rows");
        assert_ne!(
            backend.buffer()[(0, 0)].style().bg,
            Some(surface_bg),
            "width {width}: the call line opens the entry and stays untinted"
        );
        // The band follows the call line: a surface margin, the body, a margin.
        let call_end = (1..height)
            .find(|y| backend.buffer()[(0, *y)].style().bg == Some(surface_bg))
            .ok_or("the body must paint the surface below the call line")?;
        assert!(
            call_end >= 1,
            "width {width}: the band starts below the call"
        );
        for y in 0..call_end {
            assert_ne!(
                backend.buffer()[(0, y)].style().bg,
                Some(surface_bg),
                "width {width}: call row {y} stays untinted"
            );
        }
        for y in call_end..height {
            assert_eq!(
                backend.buffer()[(0, y)].style().bg,
                Some(surface_bg),
                "width {width}: every band row paints the code surface (row {y})"
            );
        }
    }
    Ok(())
}

/// WHEN a tool call carries a status indicator, THE call line SHALL wrap inside
/// the pane: the indicator takes two columns on row 0, and no row SHALL reach
/// the pane's last two columns.
#[test]
fn test_transcript_layout_status_indicator_wraps_within_pane_width() -> Result<()> {
    // -- Setup & Fixtures: a summary far wider than the pane.
    let call = ToolCall::new(
        ToolName("tool".to_string()),
        "",
        "",
        CallLine {
            summary: format!("\u{2192} {}", "x".repeat(200)),
        },
    );
    let entry = TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call,
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    });

    for width in [80usize, 120] {
        // -- Exec
        let rows = RatatuiBackend::new(0, 0).measure(&entry, width);
        let columns = u16::try_from(width).map_err(|_| "width must fit u16")?;
        let height = u16::try_from(rows).map_err(|_| "rows must fit u16")?;
        let mut backend = RatatuiBackend::new(columns, height);
        backend.render(&entry, RowRect::new(0, width, rows), &Theme::default());

        // -- Check: every row fits inside the pane's wrap budget, and the
        // planner adds no gap rows ahead of a first entry.
        assert!(rows > 1, "width {width}: the summary must wrap");
        for y in 0..height {
            let text: String = (0..columns)
                .map(|x| backend.buffer()[(x, y)].symbol().to_string())
                .collect();
            assert!(
                text.trim_end().chars().count() <= width - 2,
                "width {width}: row {y} must keep the right margin clear; got {:?}",
                text.trim_end()
            );
        }
        let plan = LayoutPlanner::plan(std::slice::from_ref(&entry), width, &backend);
        assert_eq!(
            plan.total_rows, rows,
            "width {width}: plan and measure agree"
        );
    }
    Ok(())
}

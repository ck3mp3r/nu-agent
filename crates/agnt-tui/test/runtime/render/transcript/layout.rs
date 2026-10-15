use super::*;

#[test]
fn fill_code_layout_includes_margin_rows_matching_accounting() -> Result<()> {
    use crate::state::TranscriptStore;
    use crate::tui_renderer::layout;

    let block = nu_tool_block("ls | where size > 1mb\n| select name type\n| sort-by modified");
    for width in [80usize, 120usize] {
        let rendered = layout(&block, &frame_ctx(width), &TuiTheme::default()).len();

        // -- Exec: run the same accounting the render loop uses
        let mut store = TranscriptStore::default();
        store.push_block(block.clone());
        store.rebuild_height_index(width);

        // -- Check: accounting includes the 2 margin rows layout() injects
        assert_eq!(
            store.total_visual_rows(),
            rendered,
            "width {width}: visual accounting must match layout() row count incl. margins"
        );
    }
    Ok(())
}

#[test]
fn status_indicator_wraps_within_pane_width() -> Result<()> {
    use crate::tui_renderer::layout;

    // A tool whose row-0 summary wraps at the pane width. With a status
    // indicator on row 0, the renderer must shrink the wrap budget by 2
    // columns so row 0 fits the pane and ratatui does not re-wrap it into an
    // extra visual row.
    let block = agnt_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: format!("\u{2192} {}", "x".repeat(200)),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(agnt_core::transcript::renderer::ItemStatus::InProgress),
    };
    for width in [80usize, 120usize] {
        let lines = layout(&block, &frame_ctx(width), &TuiTheme::default());
        for line in &lines {
            let line_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(
                line_width <= width,
                "width {width}: rendered line {line_width} exceeds pane width {width}"
            );
        }
    }
    Ok(())
}

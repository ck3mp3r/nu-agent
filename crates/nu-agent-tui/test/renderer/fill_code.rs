use super::support::{fill_code_tool_block_with_status, frame_ctx, lines_text};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::layout;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== Fill::Code margin rows and row-0 tint (task 69bd5698) =========

#[test]
fn layout_tool_code_preview_inserts_margins_with_surface0_bg() -> Result<()> {
    // -- Setup & Fixtures
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "ls | select name".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    // Fill::Code inserts a top margin, the content rows, then a bottom margin.
    assert!(
        lines.len() >= 4,
        "Fill::Code must insert margin rows above and below; got {} rows",
        lines.len()
    );
    let surface0 = TuiTheme::default().surface0;
    let text = lines_text(&lines);
    assert!(
        text.contains("ls | select name"),
        "must contain command; got {text:?}"
    );

    let last = lines.len() - 1;
    // Top margin (first row) and bottom margin (last row) MUST carry the
    // surface0 fill on a real span — a bare `Line::from("")` has no span and
    // renders unfilled (task 69bd5698, bug 1).
    for (label, idx) in [("top", 0usize), ("bottom", last)] {
        let margin = lines.get(idx).ok_or("margin row must exist")?;
        assert!(
            !margin.spans.is_empty(),
            "{label} margin row must carry a span so its bg renders; got {margin:?}"
        );
        assert!(
            margin.spans.iter().any(|s| s.style.bg == Some(surface0)),
            "{label} margin row must carry a surface0 span; got {margin:?}"
        );
    }

    // The first content row (call line + status) MUST stay untinted.
    let content_row0 = lines.get(1).ok_or("first content row must exist")?;
    for span in &content_row0.spans {
        assert_ne!(
            span.style.bg,
            Some(surface0),
            "first content row (call line) must stay untinted, got {:?}",
            span.style.bg
        );
    }

    // Every inner content row (after row 0, before the bottom margin) MUST
    // carry surface0.
    for (idx, line) in lines.iter().enumerate().take(last).skip(2) {
        for span in &line.spans {
            assert_eq!(
                span.style.bg,
                Some(surface0),
                "Fill::Code content line {idx} span '{}' must carry surface0 bg, got {:?}; line: {line:?}",
                span.content,
                span.style.bg
            );
        }
    }
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block, THE first row (top margin)
/// SHALL have a span with bg surface0. Pins the renderer contract for any
/// `Fill::Code` block; in production that block is a `ToolDisplay` preview.
#[test]
fn layout_fill_code_top_margin_has_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let top = lines.first().ok_or("top margin row must exist")?;
    assert!(
        top.spans.iter().any(|s| s.style.bg == Some(surface0)),
        "top margin row must carry a surface0 span; got {top:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block, THE last row (bottom margin)
/// SHALL have a span with bg surface0. Pins the renderer contract for any
/// `Fill::Code` block; in production that block is a `ToolDisplay` preview.
#[test]
fn layout_fill_code_bottom_margin_has_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let bottom = lines.last().ok_or("bottom margin row must exist")?;
    assert!(
        bottom.spans.iter().any(|s| s.style.bg == Some(surface0)),
        "bottom margin row must carry a surface0 span; got {bottom:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block with
/// `status: Some(ItemStatus::Done)`, THE first content row (row 0 after the
/// top margin, carrying ⚙ and ✓) SHALL NOT have bg surface0 — it SHALL carry
/// the lane's row style. Pins the renderer contract for any `Fill::Code`
/// block; in production that block is a `ToolDisplay` preview, so the row-0
/// exemption is a renderer-level guarantee rather than a production path.
#[test]
fn layout_fill_code_row_zero_is_untinted() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let theme = TuiTheme::default();

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: index 0 is the top margin, index 1 is the first content row.
    let row0 = lines.get(1).ok_or("first content row must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains('✓'),
        "first content row must carry the status glyph; got {text:?}"
    );
    for span in &row0.spans {
        assert_ne!(
            span.style.bg,
            Some(theme.surface0),
            "row 0 (call line + status) must not be tinted; got {:?}",
            span.style.bg
        );
    }
    // It carries the lane row style instead (the lane style wins on row 0).
    let expected_bg = theme.row_tool.bg;
    assert_eq!(
        row0.spans
            .iter()
            .filter(|s| s.style.bg == expected_bg)
            .count(),
        row0.spans.len(),
        "row 0 must carry the lane row style on every span; got {row0:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block, THE content rows (after
/// row 0, before the bottom margin) SHALL have bg surface0. Pins the renderer
/// contract for any `Fill::Code` block; in production that block is a
/// `ToolDisplay` preview.
#[test]
fn layout_fill_code_inner_content_rows_have_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: rows 2..last are inner content rows (row 1 is row 0).
    let last = lines.len().saturating_sub(1);
    let mut checked = 0usize;
    for (idx, line) in lines.iter().enumerate().take(last).skip(2) {
        checked += 1;
        for span in &line.spans {
            assert_eq!(
                span.style.bg,
                Some(surface0),
                "inner content row {idx} span '{}' must carry surface0, got {:?}",
                span.content,
                span.style.bg
            );
        }
    }
    assert!(
        checked > 0,
        "fixture must produce at least one inner content row"
    );
    Ok(())
}

#[test]
fn layout_tool_preview_diff_renders_diff_lines() {
    // -- Setup & Fixtures
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            stats: None,
        }],
    };
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ a.rs (diff)".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("→ a.rs (diff)"),
        "row 0 must carry the call summary; got {text:?}"
    );
    assert!(
        text.contains("│new") && text.contains("│old"),
        "diff content must be projected with its line-number gutter; got {text:?}"
    );
}

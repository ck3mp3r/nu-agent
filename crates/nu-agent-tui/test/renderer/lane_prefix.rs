use super::support::{
    frame_ctx, markdown_block, row_marker, row_zero_marker, tool_block_with_status,
};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::{lane_prefix_width, layout};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== lane marker spacing tests (task 46502406) ==========

#[test]
fn layout_tool_marker_and_done_indicator_have_space_between() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(Some(ItemStatus::Done));

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("\u{2699} \u{2713}"),
        "marker and checkmark must be space-separated; got {text:?}"
    );
    assert!(
        !text.contains("\u{2699}\u{2713}"),
        "marker and checkmark must not be adjacent; got {text:?}"
    );
    Ok(())
}

#[test]
fn layout_tool_marker_and_in_progress_indicator_have_space_between() -> Result<()> {
    // -- Setup & Fixtures: now_millis 0 selects spinner frame 0 (\u{280b}).
    let block = tool_block_with_status(Some(ItemStatus::InProgress));

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("\u{2699} \u{280b}"),
        "marker and spinner must be space-separated; got {text:?}"
    );
    assert!(
        !text.contains("\u{2699}\u{280b}"),
        "marker and spinner must not be adjacent; got {text:?}"
    );
    Ok(())
}

#[test]
fn layout_tool_marker_without_status_is_two_chars() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
    assert_eq!(
        marker.content.as_ref(),
        "\u{2699} ",
        "tool marker must be icon + trailing space"
    );
    Ok(())
}

#[test]
fn layout_every_marker_lane_renders_two_char_marker() -> Result<()> {
    // -- Setup & Fixtures
    let user = markdown_block(MessageRole::User, "hi", Fill::Full);
    let compaction = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "c".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };
    let system = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "s".to_string(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec & Check
    for (block, expected) in [
        (&user, "\u{258f} "),
        (&compaction, "~ "),
        (&system, "\u{00b7} "),
    ] {
        let lines = layout(block, &frame_ctx(80), &TuiTheme::default());
        let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
        assert_eq!(
            marker.content.as_ref(),
            expected,
            "every marker lane must render icon + trailing space"
        );
    }
    Ok(())
}

#[test]
fn lane_prefix_width_matches_rendered_prefix_width() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let cursor = row0.spans.first().ok_or("cursor span must exist")?;
    let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
    let rendered_prefix = cursor.content.chars().count() + marker.content.chars().count();

    // -- Check
    assert_eq!(
        rendered_prefix,
        lane_prefix_width(),
        "lane_prefix_width() must equal the rendered cursor + marker width"
    );
    assert_eq!(lane_prefix_width(), 4, "cursor 2 + marker 2");
    Ok(())
}

// ========== lane prefix on continuation rows (task 667e4926) ==========

/// WHEN `layout()` renders a user `BlockSource::Markdown` block with multiple
/// ContentLines, THE lane prefix span of every row SHALL contain `▏`.
#[test]
fn layout_user_multi_content_line_rows_all_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: a soft break splits the paragraph into two
    // ContentLines, so the block renders as two rows at a wide width.
    let block = markdown_block(MessageRole::User, "first line\nsecond line", Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    assert!(
        lines.len() >= 2,
        "fixture must render more than one row; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a user `BlockSource::Markdown` block where the
/// first ContentLine wraps to multiple rows, THE lane prefix span of every
/// wrapped continuation row SHALL contain `▏`.
#[test]
fn layout_user_wrapped_continuation_rows_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: wraps at width 24 (20-col budget after the prefix).
    let long_text = "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd";
    let block = markdown_block(MessageRole::User, long_text, Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(24), &TuiTheme::default());

    // -- Check
    assert!(
        lines.len() >= 2,
        "prose must wrap at width 24; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "wrapped row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a tool `BlockSource::Tool` block where content wraps
/// to multiple rows, THE lane prefix span of continuation rows SHALL NOT
/// contain `⚙` (blank continuation marker).
#[test]
fn layout_tool_wrapped_continuation_rows_have_blank_marker() -> Result<()> {
    // -- Setup & Fixtures: a long summary wraps at width 80.
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: format!("\u{2192} {}", "x".repeat(200)),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: row 0 carries ⚙; every continuation row's marker is blank.
    assert!(lines.len() >= 2, "summary must wrap; got {}", lines.len());
    let row0 =
        row_marker(lines.first().ok_or("row 0 must exist")?).ok_or("marker span must exist")?;
    assert!(row0.content.contains('⚙'), "row 0 must carry the tool icon");
    for (idx, line) in lines.iter().enumerate().skip(1) {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            !marker.content.contains('⚙'),
            "tool continuation row {idx} must have a blank marker; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a compaction `BlockSource::Notice` block where
/// content wraps to multiple rows, THE lane prefix span of continuation rows
/// SHALL NOT contain `~` (blank continuation marker).
#[test]
fn layout_compaction_wrapped_continuation_rows_have_blank_marker() -> Result<()> {
    // -- Setup & Fixtures: a long notice wraps at width 80.
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compact ".repeat(30),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: row 0 carries ~; every continuation row's marker is blank.
    assert!(lines.len() >= 2, "notice must wrap; got {}", lines.len());
    let row0 =
        row_marker(lines.first().ok_or("row 0 must exist")?).ok_or("marker span must exist")?;
    assert!(
        row0.content.contains('~'),
        "row 0 must carry the compaction icon"
    );
    for (idx, line) in lines.iter().enumerate().skip(1) {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            !marker.content.contains('~'),
            "compaction continuation row {idx} must have a blank marker; got {:?}",
            marker.content
        );
    }
    Ok(())
}

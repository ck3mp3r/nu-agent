use nu_agent_core::protocol::tool_args::CallLineRender;
use nu_agent_core::transcript::items::{
    ProseMessage, Spacer, ToolInvocation, ToolResult, TranscriptEntry, TranscriptEntryKind,
};
use nu_agent_core::transcript::renderer::ItemStatus;

use super::transcript::row_needs_user_bg;
use crate::state::code_block::{code_block_line_flags, with_margin_rows};

fn user() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
}

fn assistant() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
}

fn spacer() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Spacer(Spacer),
        status: None,
    }
}

#[test]
fn user_entry_needs_user_bg() {
    let entries = vec![user()];
    assert!(row_needs_user_bg(&entries, 0));
}

#[test]
fn assistant_entry_does_not_need_user_bg() {
    let entries = vec![assistant()];
    assert!(!row_needs_user_bg(&entries, 0));
}

#[test]
fn spacer_after_user_needs_user_bg() {
    let entries = vec![user(), spacer()];
    assert!(row_needs_user_bg(&entries, 1));
}

#[test]
fn spacer_before_user_needs_user_bg() {
    let entries = vec![spacer(), user()];
    assert!(row_needs_user_bg(&entries, 0));
}

#[test]
fn spacer_between_two_users_needs_user_bg() {
    let entries = vec![user(), spacer(), user()];
    assert!(row_needs_user_bg(&entries, 1));
}

#[test]
fn spacer_not_adjacent_to_user_does_not_need_user_bg() {
    let entries = vec![assistant(), spacer(), assistant()];
    assert!(!row_needs_user_bg(&entries, 1));
}

#[test]
fn out_of_range_entry_does_not_need_user_bg() {
    let entries = vec![user()];
    assert!(!row_needs_user_bg(&entries, 5));
}

#[test]
fn non_separator_non_user_entries_do_not_need_user_bg() {
    let entries = vec![assistant()];
    assert!(!row_needs_user_bg(&entries, 0));
}

// ── row-count parity (margin rows) ──────────────────────────────────────────

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn nu_tool(args: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Tool(ToolInvocation {
            name: "nu".to_string(),
            source: "".to_string(),
            call_line: CallLineRender::CodeBlock {
                language: "nu".to_string(),
                code: args.to_string(),
            },
        }),
        status: None,
    }
}

fn edit_diff() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::ToolResult(ToolResult {
            name: String::new(),
            success: true,
            lines: vec![
                nu_agent_core::transcript::ir::ContentLine::single(
                    "+added".to_string(),
                    nu_agent_core::transcript::ir::StyleHint::DiffAdd,
                ),
                nu_agent_core::transcript::ir::ContentLine::single(
                    " context".to_string(),
                    nu_agent_core::transcript::ir::StyleHint::Normal,
                ),
                nu_agent_core::transcript::ir::ContentLine::single(
                    "-removed".to_string(),
                    nu_agent_core::transcript::ir::StyleHint::DiffRemove,
                ),
            ],
        }),
        status: None,
    }
}

fn rendered_row_count_with_margins(
    entry: &TranscriptEntry,
    width: usize,
    status: Option<ItemStatus>,
) -> Result<usize> {
    use crate::rendering::theme::TuiTheme;
    use crate::tui_renderer::TuiRenderer;
    use nu_agent_core::transcript::items::Renderable;
    use nu_agent_core::transcript::renderer::RenderContext;

    let renderer = TuiRenderer {
        theme: TuiTheme::default(),
    };
    let block = entry.to_render_block();
    let ctx = RenderContext {
        width,
        cursor: false,
        selected: false,
        status,
        now_millis: 0,
    };
    let mut cache = std::collections::HashMap::new();
    let entry_lines = renderer.render_cached(&block, &ctx, &mut cache);
    // Secondary divergence check: the renderer pre-wraps each ContentLine via
    // textwrap so no emitted Line exceeds the pane width. If any line were
    // wider, ratatui's Paragraph::line_count would re-wrap it and the visual
    // row count would diverge from textwrap's count. Assert every line fits.
    for line in &entry_lines {
        let line_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(
            line_width <= width,
            "width {width}: rendered line {line_width} exceeds pane width {width}"
        );
    }
    let flags = code_block_line_flags(entry, width, status.is_some());
    let (entry_lines, _) = with_margin_rows(entry_lines, flags);
    Ok(entry_lines.len())
}

#[test]
fn entry_visual_info_matches_rendered_rows_with_margins() -> Result<()> {
    use crate::state::{ScrollState, TranscriptStore};

    let entries = vec![
        nu_tool("ls | where size > 1mb\n| select name type\n| sort-by modified"),
        edit_diff(),
        user(),
    ];
    for width in [80usize, 120usize] {
        for entry in &entries {
            // -- Exec: compute entry_visual_info
            let mut store = TranscriptStore::default();
            store.push_transcript_item(entry.clone());
            let mut scroll = ScrollState::default();
            store.recompute_entry_visual_info(&mut scroll, width);
            let total_visual_rows = scroll
                .entry_visual_info
                .last()
                .map(|i| i.start_visual_row + i.visual_row_count)
                .ok_or("should have entry visual info")?;

            // -- Exec: render the same entry with margins
            let rendered_rows = rendered_row_count_with_margins(entry, width, None)?;

            // -- Check
            assert_eq!(
                total_visual_rows, rendered_rows,
                "width {width}: entry_visual_info must count margin rows injected by with_margin_rows"
            );
        }
    }
    Ok(())
}

#[test]
fn tail_follow_never_clips_last_entry_below_input_box() -> Result<()> {
    use crate::state::{ScrollState, TranscriptStore};

    let entry = nu_tool("ls | where size > 1mb\n| select name type\n| sort-by modified");
    for width in [80usize, 120usize] {
        let mut store = TranscriptStore::default();
        store.push_transcript_item(entry.clone());
        let mut scroll = ScrollState::default();
        store.recompute_entry_visual_info(&mut scroll, width);

        let total_visual_rows = scroll
            .entry_visual_info
            .last()
            .map(|i| i.start_visual_row + i.visual_row_count)
            .ok_or("should have entry visual info")?;
        let rendered_rows = rendered_row_count_with_margins(&entry, width, None)?;

        // A viewport smaller than the content forces tailing to scroll.
        let viewport_height = 4;
        let max_scroll = total_visual_rows.saturating_sub(viewport_height);
        let effective_offset = max_scroll; // following_tail

        // -- Check: nothing clipped — the last rendered row must be visible.
        assert!(
            effective_offset + viewport_height >= rendered_rows,
            "width {width}: tail view must show every rendered row (offset {effective_offset} + viewport {viewport_height} = {} < rendered {rendered_rows})",
            effective_offset + viewport_height
        );
    }
    Ok(())
}

#[test]
fn entry_visual_info_matches_rendered_rows_with_status_indicator() -> Result<()> {
    use crate::state::{ScrollState, TranscriptStore};

    // A non-nu tool whose row-0 content (name + "→ " + long args summary)
    // wraps at the pane width. With a status indicator on row 0, the renderer
    // must shrink the wrap budget by 2 columns so row 0 fits the pane and
    // ratatui does not re-wrap it into an extra visual row.
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Tool(ToolInvocation {
            name: "gh".to_string(),
            source: "".to_string(),
            call_line: CallLineRender::generic_json_summary(
                "{\"owner\":\"some-org\",\"repo\":\"some-repository-name\",\"number\":12345,\"labels\":[\"bug\",\"priority-high\",\"needs-review\"]}",
            ),
        }),
        status: Some(ItemStatus::InProgress),
    };
    for width in [80usize, 120usize] {
        for status in [Some(ItemStatus::InProgress), Some(ItemStatus::Done)] {
            let mut entry = entry.clone();
            entry.status = status;

            // -- Exec: compute entry_visual_info
            let mut store = TranscriptStore::default();
            store.push_transcript_item(entry.clone());
            let mut scroll = ScrollState::default();
            store.recompute_entry_visual_info(&mut scroll, width);
            let total_visual_rows = scroll
                .entry_visual_info
                .last()
                .map(|i| i.start_visual_row + i.visual_row_count)
                .ok_or("should have entry visual info")?;

            // -- Exec: render the same entry with the status indicator
            let rendered_rows = rendered_row_count_with_margins(&entry, width, status)?;

            // -- Check: the indicator must not push row 0 past the pane width,
            // and the accounting must equal the rendered row count.
            assert_eq!(
                total_visual_rows, rendered_rows,
                "width {width} status {status:?}: entry_visual_info must count the status-indicator row budget"
            );
        }
    }
    Ok(())
}

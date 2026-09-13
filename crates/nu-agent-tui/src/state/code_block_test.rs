use nu_agent_core::transcript::ir::StyleHint;
use nu_agent_core::transcript::items::{
    ProseMessage, ToolInvocation, ToolResult, TranscriptEntry, TranscriptEntryKind,
};
use ratatui::text::Line;

use super::code_block::{code_block_line_flags, with_margin_rows};

fn nu_tool(args: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Tool(ToolInvocation {
            name: "nu".to_string(),
            source: "".to_string(),
            args: args.to_string(),
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
                    StyleHint::DiffAdd,
                ),
                nu_agent_core::transcript::ir::ContentLine::single(
                    " context".to_string(),
                    StyleHint::Normal,
                ),
                nu_agent_core::transcript::ir::ContentLine::single(
                    "-removed".to_string(),
                    StyleHint::DiffRemove,
                ),
            ],
        }),
        status: None,
    }
}

fn user() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
}

// ── code_block_line_flags ───────────────────────────────────────────────────

#[test]
fn nu_tool_status_row_not_filled_code_rows_filled() {
    let entry = nu_tool("ls | select name type size");
    let flags = code_block_line_flags(&entry, 120, false);
    assert_eq!(flags.len(), 2, "status row + one code row");
    assert!(!flags[0], "status row must not be filled");
    assert!(flags[1], "code row must be filled");
}

#[test]
fn nu_tool_multi_line_command_fills_every_code_row() {
    let entry = nu_tool("ls | where size > 1mb\n| select name type\n| sort-by modified");
    let flags = code_block_line_flags(&entry, 120, false);
    assert_eq!(flags.len(), 4, "status row + 3 code rows");
    assert!(!flags[0], "status row must not be filled");
    assert!(flags[1] && flags[2] && flags[3], "all code rows filled");
}

#[test]
fn nu_tool_empty_args_has_no_code_rows() {
    let entry = nu_tool("");
    let flags = code_block_line_flags(&entry, 120, false);
    assert_eq!(flags.len(), 1, "status row only");
    assert!(!flags[0], "status row must not be filled");
}

#[test]
fn non_nu_tool_has_no_filled_rows() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Tool(ToolInvocation {
            name: "read".to_string(),
            source: "".to_string(),
            args: "{}".to_string(),
        }),
        status: None,
    };
    let flags = code_block_line_flags(&entry, 120, false);
    assert!(flags.iter().all(|&f| !f), "non-nu tool must not be filled");
}

#[test]
fn edit_diff_content_rows_all_filled() {
    let entry = edit_diff();
    let flags = code_block_line_flags(&entry, 120, false);
    assert_eq!(flags.len(), 3);
    assert!(
        flags.iter().all(|&f| f),
        "all diff-block lines (incl. context) must be filled"
    );
}

#[test]
fn non_diff_tool_result_not_filled() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::ToolResult(ToolResult {
            name: String::new(),
            success: true,
            lines: vec![nu_agent_core::transcript::ir::ContentLine::single(
                "plain".to_string(),
                StyleHint::Normal,
            )],
        }),
        status: None,
    };
    let flags = code_block_line_flags(&entry, 120, false);
    assert!(
        flags.iter().all(|&f| !f),
        "non-diff ToolResult must not be filled"
    );
}

#[test]
fn user_entry_has_no_code_block_rows() {
    let entry = user();
    let flags = code_block_line_flags(&entry, 120, false);
    assert!(flags.iter().all(|&f| !f), "user entry must not be filled");
}

// ── with_margin_rows ─────────────────────────────────────────────────────────

#[test]
fn with_margin_rows_adds_top_and_bottom_blank_rows() {
    let lines = vec![Line::from("a"), Line::from("b"), Line::from("c")];
    let flags = vec![false, true, false];
    let (out_lines, out_flags) = with_margin_rows(lines, flags);
    // [a, blank, b, blank, c] — margin rows are filled (inside the block)
    assert_eq!(out_lines.len(), 5);
    assert_eq!(out_flags, vec![false, true, true, true, false]);
    assert_eq!(out_lines[1].to_string(), "", "top margin row");
    assert_eq!(out_lines[2].to_string(), "b", "filled row preserved");
    assert_eq!(out_lines[3].to_string(), "", "bottom margin row");
}

#[test]
fn with_margin_rows_no_fill_returns_unchanged() {
    let lines = vec![Line::from("a"), Line::from("b")];
    let flags = vec![false, false];
    let (out_lines, out_flags) = with_margin_rows(lines, flags);
    assert_eq!(out_lines.len(), 2);
    assert_eq!(out_flags, vec![false, false]);
}

#[test]
fn with_margin_rows_contiguous_run_gets_single_margin_pair() {
    let lines = vec![Line::from("a"), Line::from("b"), Line::from("c")];
    let flags = vec![false, true, true];
    let (out_lines, out_flags) = with_margin_rows(lines, flags);
    // [a, blank, b, c, blank] — margin rows are filled (inside the block)
    assert_eq!(out_lines.len(), 5);
    assert_eq!(out_flags, vec![false, true, true, true, true]);
}

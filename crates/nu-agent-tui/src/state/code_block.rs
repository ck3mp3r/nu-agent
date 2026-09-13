//! Code-block detection and margin-row injection shared by the transcript
//! renderer and the visual-row accounting in [`TranscriptStore`].
//!
//! Both paths must agree on how many visual rows an entry occupies. The
//! renderer injects margin rows around filled code-block runs at render time
//! ([`with_margin_rows`]); the row-accounting path
//! ([`TranscriptStore::recompute_entry_visual_info`]) must count those same
//! margin rows so `total_visual_rows` matches the rendered output. Keeping
//! both functions here guarantees the two counts agree by construction rather
//! than by coincidence.

use nu_agent_core::transcript::ir::{ContentLine, StyleHint};
use nu_agent_core::transcript::items::{Renderable, TranscriptEntry, TranscriptEntryKind};
use ratatui::text::Line;

use crate::tui_renderer::{lane_prefix_width, wrap_prose};

/// Width of the status indicator span (icon char + trailing space) appended to
/// row 0 when a status is present.
pub const STATUS_INDICATOR_WIDTH: usize = 2;

/// Effective wrap budget for a ContentLine's text given the pane width and
/// whether a status indicator is rendered on row 0. Both the renderer and the
/// visual-row accounting must derive their wrap width from this so row counts
/// agree by construction: the indicator occupies 2 columns on row 0, so the
/// content budget must shrink by that much or the row overflows the pane and
/// ratatui re-wraps it into an extra visual row the accounting misses.
pub fn content_wrap_width(width: usize, has_status: bool) -> usize {
    let indicator = if has_status {
        STATUS_INDICATOR_WIDTH
    } else {
        0
    };
    width.saturating_sub(lane_prefix_width() + indicator).max(1)
}

/// Compute per-rendered-line flags indicating whether each line of the entry's
/// rendered block is a code-block content row that should receive the
/// full-width background fill. Returns one flag per rendered line (pre-wrap).
///
/// - A nu `ToolInvocation` entry: row 0 (status row) is untinted; rows 1+ are
///   code-block rows.
/// - An edit `ToolResult` display entry: the diff content lines (those carrying
///   Diff* StyleHints) are tinted; label/stats lines are not.
/// - All other entries: no code-block rows.
pub fn code_block_line_flags(entry: &TranscriptEntry, width: usize, has_status: bool) -> Vec<bool> {
    let block = entry.to_render_block();
    let content_lines: Vec<ContentLine> = if let Some(md) = &block.markdown {
        crate::markdown::render_markdown_lines(md, Some(width as u16))
    } else {
        block.lines
    };
    let effective_width = content_wrap_width(width, has_status);

    let mut flags = Vec::new();
    match &entry.kind {
        TranscriptEntryKind::Tool(inv) if inv.name == "nu" => {
            for (idx, line) in content_lines.iter().enumerate() {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                let text_width = effective_width.saturating_sub(line.hang_indent).max(1);
                let rows = wrap_prose(&text, text_width).len().max(1);
                for _ in 0..rows {
                    flags.push(idx > 0);
                }
            }
        }
        TranscriptEntryKind::ToolResult(_) => {
            // A ToolResult entry that carries any Diff* hint is a diff content
            // block (pushed by push_tool_display_lines); fill every line of it.
            // Label/stats lines are separate entries with no Diff* hints and
            // stay untinted.
            let is_diff_block = content_lines.iter().any(|line| {
                line.spans.iter().any(|s| {
                    matches!(
                        s.hint,
                        StyleHint::DiffAdd | StyleHint::DiffRemove | StyleHint::DiffHunk
                    )
                })
            });
            for line in content_lines.iter() {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                let text_width = effective_width.saturating_sub(line.hang_indent).max(1);
                let rows = wrap_prose(&text, text_width).len().max(1);
                flags.extend(std::iter::repeat_n(is_diff_block, rows));
            }
        }
        _ => {
            for line in content_lines.iter() {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                let text_width = effective_width.saturating_sub(line.hang_indent).max(1);
                let rows = wrap_prose(&text, text_width).len().max(1);
                flags.extend(std::iter::repeat_n(false, rows));
            }
        }
    }
    flags
}

/// Number of margin rows [`with_margin_rows`] will inject for the given flags:
/// 2 per contiguous run of filled rows (one top, one bottom).
pub fn margin_row_count(flags: &[bool]) -> usize {
    let mut runs = 0usize;
    let mut prev_filled = false;
    for &flag in flags {
        if flag && !prev_filled {
            runs += 1;
        }
        prev_filled = flag;
    }
    runs * 2
}

/// Insert one blank filled row immediately above and below each contiguous
/// run of filled code-block rows, so the background block has top/bottom
/// margin rows INSIDE the block (internal padding around the text). Returns
/// the padded lines and matching flags; the inserted margin rows are filled
/// (flag true) so they share the block background.
pub fn with_margin_rows(
    lines: Vec<Line<'static>>,
    flags: Vec<bool>,
) -> (Vec<Line<'static>>, Vec<bool>) {
    let mut out_lines = Vec::with_capacity(lines.len() + 2);
    let mut out_flags = Vec::with_capacity(flags.len() + 2);
    let mut prev_filled = false;
    for (line, flag) in lines.into_iter().zip(flags) {
        if flag && !prev_filled {
            // Start of a filled run: insert a filled top margin row.
            out_lines.push(Line::from(""));
            out_flags.push(true);
        } else if !flag && prev_filled {
            // End of a filled run: insert a filled bottom margin row.
            out_lines.push(Line::from(""));
            out_flags.push(true);
        }
        out_lines.push(line);
        out_flags.push(flag);
        prev_filled = flag;
    }
    if prev_filled {
        // Trailing filled run: insert a filled bottom margin row.
        out_lines.push(Line::from(""));
        out_flags.push(true);
    }
    (out_lines, out_flags)
}

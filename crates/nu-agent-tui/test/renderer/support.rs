use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{
    Block, BlockSource, ContentKind, Display, DisplaySection, Fill, Lane, MessageRole, ToolName,
};
use nu_agent_core::transcript::items::Message;
use nu_agent_core::transcript::renderer::{FrameContext, ItemStatus, Renderable};

pub(crate) fn frame_ctx(width: usize) -> FrameContext {
    FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

pub(crate) fn markdown_block(role: MessageRole, markdown: &str, fill: Fill) -> Block {
    let msg = Message {
        role,
        markdown: markdown.to_string(),
    };
    Block {
        source: msg.source(),
        lane: msg.lane(),
        fill,
        status: None,
    }
}

pub(crate) fn lines_text(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect()
}

/// A `Fill::None` tool block whose preview is a single diff section, so the
/// projected lines carry `DiffTint` backgrounds. The lane is the tool lane
/// (`⚙`), whose row style has no background — so any background on a diff row
/// comes from the tint alone.
pub(crate) fn diff_tint_block(diff: &str) -> Block {
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: diff.to_string(),
            stats: None,
        }],
    };
    Block {
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
    }
}

/// The rows of `lines` whose joined text contains `needle`.
pub(crate) fn rows_containing<'a>(
    lines: &'a [ratatui::text::Line<'static>],
    needle: &str,
) -> Vec<&'a ratatui::text::Line<'static>> {
    lines
        .iter()
        .filter(|line| {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            text.contains(needle)
        })
        .collect()
}

/// The content spans of a rendered row: everything after the 2-span lane
/// prefix (cursor slot + marker). The lane prefix is renderer chrome, not part
/// of the `ContentLine`, so it never carries a diff tint.
pub(crate) fn content_spans<'a>(
    line: &'a ratatui::text::Line<'static>,
) -> &'a [ratatui::text::Span<'static>] {
    line.spans.get(2..).unwrap_or(&[])
}

/// A `Fill::Code` block with a status indicator: row 0 carries the call line
/// and the done glyph, on the tool lane (not tinted).
///
/// This pins the RENDERER contract for any `Fill::Code` block — margins above
/// and below, row 0 untinted at span level. In production the `Fill::Code`
/// producer is a `ToolDisplay` preview block, never a Tool block: the Tool
/// block keeps `Fill::None` so its call line gets neither the code surface nor
/// a margin row (task 4b70df9b).
pub(crate) fn fill_code_tool_block_with_status() -> Block {
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls".to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "→ ls".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: Some(ItemStatus::Done),
    }
}

pub(crate) fn tool_code_block(code: &str) -> Block {
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: code.to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: code.to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    }
}

pub(crate) fn tool_diff_block(diff: &str) -> Block {
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: diff.to_string(),
            stats: None,
        }],
    };
    Block {
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
    }
}

pub(crate) fn spacer_block() -> Block {
    Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

/// Find the first rendered span whose text contains `needle`.
pub(crate) fn span_containing<'a>(
    lines: &'a [ratatui::text::Line<'static>],
    needle: &str,
) -> Option<&'a ratatui::text::Span<'static>> {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains(needle))
}

/// Number of leading space characters on a rendered row.
pub(crate) fn leading_spaces(line: &ratatui::text::Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
        .chars()
        .take_while(|c| *c == ' ')
        .count()
}

/// Build a Tool block with the given status; no preview.
pub(crate) fn tool_block_with_status(status: Option<ItemStatus>) -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "\u{2192} a".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("\u{2699}"),
        fill: Fill::None,
        status,
    }
}

/// The marker span is index 1 of the first rendered line (index 0 is the
/// 2-char cursor slot).
pub(crate) fn row_zero_marker<'a>(
    lines: &'a [ratatui::text::Line<'static>],
) -> Option<&'a ratatui::text::Span<'static>> {
    lines.first()?.spans.get(1)
}

/// The 2-char lane marker span of any rendered row: index 0 is the
/// cursor/blank slot, index 1 is the marker slot (row 0 marker or the
/// continuation marker).
pub(crate) fn row_marker<'a>(
    line: &'a ratatui::text::Line<'static>,
) -> Option<&'a ratatui::text::Span<'static>> {
    line.spans.get(1)
}

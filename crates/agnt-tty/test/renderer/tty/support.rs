//! Shared fixtures for the TTY renderer test modules.
//!
//! Holds the imports and the block builders every topical tty test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::renderer::tty::layout;
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::ir::{
    Block, BlockSource, ContentKind, ContentLine, Display, DisplaySection, Fill, Lane, MessageRole,
    NoticeKind, StyleHint, ToolName,
};
pub(super) use agnt_core::transcript::renderer::{FrameContext, ItemStatus};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

pub(super) fn ctx() -> FrameContext {
    FrameContext {
        width: 80,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

/// Render with color on — the common case for tests asserting styled output.
pub(super) fn render(block: &Block) -> String {
    layout(block, &ctx(), true)
}

/// Render with color off — for tests asserting plain, ANSI-free output.
pub(super) fn render_plain(block: &Block) -> String {
    layout(block, &ctx(), false)
}

pub(super) fn markdown_block(role: MessageRole, markdown: &str) -> Block {
    let (lane, fill) = match role {
        MessageRole::User => (Lane::Marker("╎"), Fill::None),
        MessageRole::Assistant => (Lane::Blank, Fill::None),
    };
    Block {
        source: BlockSource::Markdown {
            role,
            markdown: markdown.to_string(),
        },
        lane,
        fill,
        status: None,
    }
}

pub(super) fn tool_block(name: &str, summary: &str) -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName(name.to_string()),
            call: CallLine {
                summary: summary.to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn notice_block(text: &str) -> Block {
    Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: text.to_string(),
        },
        lane: Lane::Marker("·"),
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn spacer_block() -> Block {
    Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

/// A ToolDisplay block: pre-projected tool output on `Lane::Blank`.
pub(super) fn tool_display_block(lines: Vec<ContentLine>) -> Block {
    Block {
        source: BlockSource::ToolDisplay { lines },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

/// Render at a specific wall-clock time, for spinner-frame assertions.
pub(super) fn render_at(block: &Block, now_millis: u128) -> String {
    let ctx = FrameContext {
        width: 80,
        now_millis,
        cursor: false,
        selected: false,
    };
    layout(block, &ctx, true)
}

pub(super) fn diff_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: CallLine {
                summary: "edit file".to_string(),
            },
            preview: Some(Display {
                title: Some("edit file".to_string()),
                sections: vec![DisplaySection {
                    label: Some("diff".to_string()),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "@@ -1,2 +1,2 @@\n--- a.rs\n+++ b.rs\n+added\n-removed\nunchanged"
                        .to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn code_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "nu".to_string(),
            },
            preview: Some(Display {
                title: Some("nu".to_string()),
                sections: vec![DisplaySection {
                    label: Some("nu".to_string()),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls | where size > 1mb\n| sort-by modified".to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn plain_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("http".to_string()),
            call: CallLine {
                summary: "http".to_string(),
            },
            preview: Some(Display {
                title: Some("http".to_string()),
                sections: vec![DisplaySection {
                    label: Some("body".to_string()),
                    kind: ContentKind::Plain,
                    content: "key: value".to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

// endregion: --- Test Support

use super::*;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::ir::{
    Block, BlockFamily, BlockSource, ContentLine, Fill, Lane, MessageRole, NoticeKind, StyleHint,
    ToolName,
};

// ---------------------------------------------------------------------------
// SpacerStateMachine transition rules (task 4609782e)
// ---------------------------------------------------------------------------

/// Separator counts between two adjacent blocks. Each row feeds the
/// predecessor into a fresh machine — a `None` predecessor is the first block
/// of a transcript — then asks for the incoming block's count.
#[test]
fn separator_counts_follow_block_pair_table() {
    struct Case {
        name: &'static str,
        predecessor: Option<(BlockFamily, bool)>,
        incoming: BlockFamily,
        incoming_has_filled: bool,
        expected: usize,
    }

    // -- Setup & Fixtures
    let cases = [
        // Zero separators
        Case {
            name: "no separator before the first block",
            predecessor: None,
            incoming: BlockFamily::User,
            incoming_has_filled: false,
            expected: 0,
        },
        Case {
            name: "consecutive tool calls stay in one block",
            predecessor: Some((BlockFamily::Tool, false)),
            incoming: BlockFamily::Tool,
            incoming_has_filled: false,
            expected: 0,
        },
        Case {
            name: "a tool display continues its tool call block",
            predecessor: Some((BlockFamily::Tool, false)),
            incoming: BlockFamily::ToolDisplay,
            incoming_has_filled: false,
            expected: 0,
        },
        Case {
            name: "consecutive tool display rows stay together",
            predecessor: Some((BlockFamily::ToolDisplay, true)),
            incoming: BlockFamily::ToolDisplay,
            incoming_has_filled: true,
            expected: 0,
        },
        Case {
            name: "an unfilled display folds into the tool block",
            predecessor: Some((BlockFamily::ToolDisplay, false)),
            incoming: BlockFamily::Tool,
            incoming_has_filled: false,
            expected: 0,
        },
        // One separator
        Case {
            name: "a filled display needs one row before the next tool",
            predecessor: Some((BlockFamily::ToolDisplay, true)),
            incoming: BlockFamily::Tool,
            incoming_has_filled: false,
            expected: 1,
        },
        Case {
            name: "assistant prose continues the tool run",
            predecessor: Some((BlockFamily::Tool, false)),
            incoming: BlockFamily::Assistant,
            incoming_has_filled: false,
            expected: 1,
        },
        Case {
            name: "a tool call after prose needs one row",
            predecessor: Some((BlockFamily::Assistant, false)),
            incoming: BlockFamily::Tool,
            incoming_has_filled: false,
            expected: 1,
        },
        // Two separators
        Case {
            name: "two prose blocks get a closing and a starting row",
            predecessor: Some((BlockFamily::User, false)),
            incoming: BlockFamily::User,
            incoming_has_filled: false,
            expected: 2,
        },
        Case {
            name: "a notice to prose transition needs two rows",
            predecessor: Some((BlockFamily::Notice, false)),
            incoming: BlockFamily::User,
            incoming_has_filled: false,
            expected: 2,
        },
        Case {
            name: "the banner to prose transition needs two rows",
            predecessor: Some((BlockFamily::Banner, false)),
            incoming: BlockFamily::User,
            incoming_has_filled: false,
            expected: 2,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let mut sm = SpacerStateMachine::default();
        if let Some((family, has_filled)) = case.predecessor {
            sm.separators_for(family, has_filled);
        }
        let sep = sm.separators_for(case.incoming, case.incoming_has_filled);
        assert_eq!(sep, case.expected, "{}", case.name);
    }
}

/// A seeded machine answers the first visible block from the predecessor that
/// scrolled above the render slice.
#[test]
fn seeded_machine_separator_counts_table() {
    // -- Setup & Fixtures
    let cases = [
        (
            BlockFamily::User,
            false,
            BlockFamily::Tool,
            false,
            2,
            "seeded predecessor drives the first count",
        ),
        (
            BlockFamily::ToolDisplay,
            true,
            BlockFamily::Tool,
            false,
            1,
            "seeded filled display separates the next tool call",
        ),
    ];

    // -- Exec & Check
    for (prev_family, prev_has_filled, incoming, incoming_has_filled, expected, name) in cases {
        let mut sm = SpacerStateMachine::seeded(prev_family, prev_has_filled);
        let sep = sm.separators_for(incoming, incoming_has_filled);
        assert_eq!(sep, expected, "{name}");
    }
}

// ---------------------------------------------------------------------------
// SpacerStateMachine trailing separator (task 670e0292)
// ---------------------------------------------------------------------------

/// Trailing separator counts after the last block: a user turn closes with one
/// row, every other family ends flush, and an empty machine has nothing to
/// close.
#[test]
fn trailing_separator_counts_table() {
    // -- Setup & Fixtures
    let cases = [
        (
            Some(BlockFamily::User),
            1,
            "a user turn closes with one separator row",
        ),
        (
            Some(BlockFamily::Assistant),
            0,
            "assistant carries no full-width fill, so it needs no trailing row",
        ),
        (
            Some(BlockFamily::Tool),
            0,
            "tool carries no full-width fill, so it needs no trailing row",
        ),
        (
            Some(BlockFamily::ToolDisplay),
            0,
            "tool display carries no full-width fill, so it needs no trailing row",
        ),
        (
            Some(BlockFamily::Notice),
            0,
            "notice carries no full-width fill, so it needs no trailing row",
        ),
        (
            Some(BlockFamily::Banner),
            0,
            "banner carries no full-width fill, so it needs no trailing row",
        ),
        (None, 0, "no block seen, nothing to close"),
    ];

    // -- Exec & Check
    for (last_family, expected, name) in cases {
        let mut sm = SpacerStateMachine::default();
        if let Some(family) = last_family {
            sm.separators_for(family, false);
        }
        assert_eq!(sm.trailing_separators(), expected, "{name}");
    }
}

// ---------------------------------------------------------------------------
// BlockSource::family() and has_diff_content()
// ---------------------------------------------------------------------------

/// `BlockSource::family()` classifies every variant.
#[test]
fn family_classifies_every_variant() {
    // -- Setup & Fixtures
    let cases = [
        (
            BlockSource::Markdown {
                role: MessageRole::User,
                markdown: "hi".to_string(),
            },
            BlockFamily::User,
        ),
        (
            BlockSource::Markdown {
                role: MessageRole::Assistant,
                markdown: "hi".to_string(),
            },
            BlockFamily::Assistant,
        ),
        (
            BlockSource::Tool {
                name: ToolName("read".to_string()),
                call: CallLine {
                    summary: "→ a".to_string(),
                },
                preview: None,
            },
            BlockFamily::Tool,
        ),
        (
            BlockSource::ToolDisplay { lines: vec![] },
            BlockFamily::ToolDisplay,
        ),
        (
            BlockSource::Notice {
                kind: NoticeKind::System,
                text: "n".to_string(),
            },
            BlockFamily::Notice,
        ),
        (
            BlockSource::Banner {
                text: "b".to_string(),
            },
            BlockFamily::Banner,
        ),
    ];

    // -- Exec & Check
    for (source, expected) in cases {
        assert_eq!(source.family(), expected, "{source:?}");
    }
}

// ---------------------------------------------------------------------------
// Filled-content predicates
// ---------------------------------------------------------------------------

/// Filled-content detection: `BlockSource::has_diff_content()` reports diff
/// lines, and `Block::has_filled_content()` reports the code fill or diff
/// content. Both terms are required: the nu preview is a `Fill::Code` block
/// with `MdCode*` lines, the completion-path diff display is a `Fill::None`
/// block with `Diff*` lines, and title/label/stats text blocks carry neither.
/// A `Fill::Full` block (a user prompt) is not a code/diff region: the
/// predicate keys on `Fill::Code`, so the user fill stays out of the
/// filled-region rule.
#[test]
fn filled_content_predicates_table() {
    // -- Setup & Fixtures
    let source_cases = [
        (
            BlockSource::ToolDisplay {
                lines: vec![ContentLine::single(
                    "+added".to_string(),
                    StyleHint::DiffAdd,
                )],
            },
            true,
            "DiffAdd line → true",
        ),
        (
            BlockSource::ToolDisplay {
                lines: vec![ContentLine::single(
                    "@@ -1 +1 @@".to_string(),
                    StyleHint::DiffHunk,
                )],
            },
            true,
            "DiffHunk line → true",
        ),
        (
            BlockSource::ToolDisplay {
                lines: vec![ContentLine::single("plain".to_string(), StyleHint::Normal)],
            },
            false,
            "Normal line → false",
        ),
        (
            BlockSource::Notice {
                kind: NoticeKind::System,
                text: "n".to_string(),
            },
            false,
            "non-ToolDisplay → false",
        ),
    ];

    let block_cases = [
        (
            Block {
                source: BlockSource::ToolDisplay {
                    lines: vec![ContentLine::single(
                        "ls".to_string(),
                        StyleHint::MdCodePlain,
                    )],
                },
                lane: Lane::Blank,
                fill: Fill::Code,
                status: None,
            },
            true,
            "Fill::Code with MdCode* lines → true",
        ),
        (
            Block {
                source: BlockSource::ToolDisplay {
                    lines: vec![ContentLine::single(
                        "+added".to_string(),
                        StyleHint::DiffAdd,
                    )],
                },
                lane: Lane::Blank,
                fill: Fill::None,
                status: None,
            },
            true,
            "Fill::None with a DiffAdd line → true",
        ),
        (
            Block {
                source: BlockSource::ToolDisplay {
                    lines: vec![ContentLine::single("plain".to_string(), StyleHint::Normal)],
                },
                lane: Lane::Blank,
                fill: Fill::None,
                status: None,
            },
            false,
            "Fill::None with Normal lines → false",
        ),
        (
            Block {
                source: BlockSource::Markdown {
                    role: MessageRole::User,
                    markdown: "hi".to_string(),
                },
                lane: Lane::Marker("▏"),
                fill: Fill::Full,
                status: None,
            },
            false,
            "Fill::Full is not the code fill → false",
        ),
    ];

    // -- Exec & Check
    for (source, expected, name) in source_cases {
        assert_eq!(source.has_diff_content(), expected, "{name}");
    }
    for (block, expected, name) in block_cases {
        assert_eq!(block.has_filled_content(), expected, "{name}");
    }
}

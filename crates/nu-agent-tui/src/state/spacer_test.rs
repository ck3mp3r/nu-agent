use super::*;
use nu_agent_core::transcript::ir::BlockFamily;

// ---------------------------------------------------------------------------
// SpacerStateMachine transition rules (task 4609782e)
// ---------------------------------------------------------------------------

#[test]
fn first_block_has_zero_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    let sep = sm.separators_for(BlockFamily::User, false);

    // -- Check
    assert_eq!(sep, 0, "no separator before the first block");
}

#[test]
fn two_tool_blocks_without_preview_have_zero_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Tool, false);
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(sep, 0, "consecutive tool calls stay in one block");
}

#[test]
fn tool_then_tool_display_has_zero_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Tool, false);
    let sep = sm.separators_for(BlockFamily::ToolDisplay, false);

    // -- Check
    assert_eq!(sep, 0, "a tool display continues its tool call block");
}

#[test]
fn two_tool_displays_have_zero_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::ToolDisplay, true);
    let sep = sm.separators_for(BlockFamily::ToolDisplay, true);

    // -- Check
    assert_eq!(sep, 0, "consecutive tool display rows stay together");
}

#[test]
fn tool_display_with_diff_then_tool_has_one_separator() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::ToolDisplay, true);
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(
        sep, 1,
        "a filled display needs one row before the next tool"
    );
}

#[test]
fn tool_display_without_diff_then_tool_has_zero_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::ToolDisplay, false);
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(sep, 0, "an unfilled display folds into the tool block");
}

#[test]
fn tool_then_assistant_has_one_separator() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Tool, false);
    let sep = sm.separators_for(BlockFamily::Assistant, false);

    // -- Check
    assert_eq!(sep, 1, "assistant prose continues the tool run");
}

#[test]
fn assistant_then_tool_has_one_separator() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Assistant, false);
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(sep, 1, "a tool call after prose needs one row");
}

#[test]
fn two_user_blocks_have_two_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::User, false);
    let sep = sm.separators_for(BlockFamily::User, false);

    // -- Check
    assert_eq!(sep, 2, "two prose blocks get a closing and a starting row");
}

#[test]
fn notice_then_user_has_two_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Notice, false);
    let sep = sm.separators_for(BlockFamily::User, false);

    // -- Check
    assert_eq!(sep, 2, "a notice to prose transition needs two rows");
}

#[test]
fn banner_then_user_has_two_separators() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::Banner, false);
    let sep = sm.separators_for(BlockFamily::User, false);

    // -- Check
    assert_eq!(sep, 2, "the banner to prose transition needs two rows");
}

#[test]
fn seeded_machine_uses_predecessor_for_first_separator() {
    // -- Setup & Fixtures: the render slice starts on a tool block whose
    // predecessor (scrolled above) was user prose. The first visible block
    // must still get its separator count.
    let mut sm = SpacerStateMachine::seeded(BlockFamily::User, false);

    // -- Exec
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(sep, 2, "seeded predecessor drives the first count");
}

#[test]
fn seeded_machine_with_diff_display_predecessor_separates_next_tool() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::seeded(BlockFamily::ToolDisplay, true);

    // -- Exec
    let sep = sm.separators_for(BlockFamily::Tool, false);

    // -- Check
    assert_eq!(sep, 1, "seeded filled display separates the next tool call");
}

// ---------------------------------------------------------------------------
// SpacerStateMachine trailing separator (task 670e0292)
// ---------------------------------------------------------------------------

#[test]
fn trailing_separators_after_user_block_is_one() {
    // -- Setup & Fixtures
    let mut sm = SpacerStateMachine::default();

    // -- Exec
    sm.separators_for(BlockFamily::User, false);
    let trailing = sm.trailing_separators();

    // -- Check
    assert_eq!(trailing, 1, "a user turn closes with one separator row");
}

#[test]
fn trailing_separators_after_non_user_block_is_zero() {
    // -- Exec & Check
    for family in [
        BlockFamily::Assistant,
        BlockFamily::Tool,
        BlockFamily::ToolDisplay,
        BlockFamily::Notice,
        BlockFamily::Banner,
    ] {
        let mut sm = SpacerStateMachine::default();
        sm.separators_for(family, false);
        assert_eq!(
            sm.trailing_separators(),
            0,
            "{family:?} carries no full-width fill, so it needs no trailing row"
        );
    }
}

#[test]
fn trailing_separators_on_empty_machine_is_zero() {
    // -- Setup & Fixtures
    let sm = SpacerStateMachine::default();

    // -- Exec & Check
    assert_eq!(
        sm.trailing_separators(),
        0,
        "no block seen, nothing to close"
    );
}

// ---------------------------------------------------------------------------
// BlockSource::family() and has_diff_content()
// ---------------------------------------------------------------------------

use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{
    Block, BlockSource, ContentKind, ContentLine, Display, DisplaySection, Fill, Lane, MessageRole,
    NoticeKind, Span, StyleHint, ToolName,
};

#[test]
fn family_classifies_every_variant() {
    // -- Setup & Fixtures
    let user = BlockSource::Markdown {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };
    let assistant = BlockSource::Markdown {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
    };
    let tool = BlockSource::Tool {
        name: ToolName("read".to_string()),
        call: CallLine {
            summary: "→ a".to_string(),
        },
        preview: None,
    };
    let tool_display = BlockSource::ToolDisplay { lines: vec![] };
    let notice = BlockSource::Notice {
        kind: NoticeKind::System,
        text: "n".to_string(),
    };
    let banner = BlockSource::Banner {
        text: "b".to_string(),
    };

    // -- Exec & Check
    assert_eq!(user.family(), BlockFamily::User);
    assert_eq!(assistant.family(), BlockFamily::Assistant);
    assert_eq!(tool.family(), BlockFamily::Tool);
    assert_eq!(tool_display.family(), BlockFamily::ToolDisplay);
    assert_eq!(notice.family(), BlockFamily::Notice);
    assert_eq!(banner.family(), BlockFamily::Banner);
}

#[test]
fn has_diff_content_is_true_only_for_diff_tool_display() {
    // -- Setup & Fixtures
    let diff_display = BlockSource::ToolDisplay {
        lines: vec![ContentLine::single(
            "+added".to_string(),
            StyleHint::DiffAdd,
        )],
    };
    let hunk_display = BlockSource::ToolDisplay {
        lines: vec![ContentLine::single(
            "@@ -1 +1 @@".to_string(),
            StyleHint::DiffHunk,
        )],
    };
    let plain_display = BlockSource::ToolDisplay {
        lines: vec![ContentLine::single("plain".to_string(), StyleHint::Normal)],
    };
    let notice = BlockSource::Notice {
        kind: NoticeKind::System,
        text: "n".to_string(),
    };
    let _ = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Plain,
            content: "c".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert!(diff_display.has_diff_content(), "DiffAdd line → true");
    assert!(hunk_display.has_diff_content(), "DiffHunk line → true");
    assert!(!plain_display.has_diff_content(), "Normal line → false");
    assert!(!notice.has_diff_content(), "non-ToolDisplay → false");
    let _ = Span::normal("x".to_string());
}

// ---------------------------------------------------------------------------
// Block::has_filled_content()
// ---------------------------------------------------------------------------

/// A block renders a filled region when it carries the code fill, or when its
/// source carries diff content. Both terms are required: the nu preview is a
/// `Fill::Code` block with `MdCode*` lines, the completion-path diff display
/// is a `Fill::None` block with `Diff*` lines, and title/label/stats text
/// blocks carry neither.
#[test]
fn block_has_filled_content_covers_code_fill_and_diff_lines() {
    // -- Setup & Fixtures
    let code_fill_block = Block {
        source: BlockSource::ToolDisplay {
            lines: vec![ContentLine::single(
                "ls".to_string(),
                StyleHint::MdCodePlain,
            )],
        },
        lane: Lane::Blank,
        fill: Fill::Code,
        status: None,
    };
    let diff_lines_block = Block {
        source: BlockSource::ToolDisplay {
            lines: vec![ContentLine::single(
                "+added".to_string(),
                StyleHint::DiffAdd,
            )],
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };
    let plain_text_block = Block {
        source: BlockSource::ToolDisplay {
            lines: vec![ContentLine::single("plain".to_string(), StyleHint::Normal)],
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec & Check
    assert!(
        code_fill_block.has_filled_content(),
        "Fill::Code with MdCode* lines → true"
    );
    assert!(
        diff_lines_block.has_filled_content(),
        "Fill::None with a DiffAdd line → true"
    );
    assert!(
        !plain_text_block.has_filled_content(),
        "Fill::None with Normal lines → false"
    );
}

/// A `Fill::Full` block (a user prompt) is not a code/diff region: the
/// predicate keys on `Fill::Code`, so the user fill stays out of the
/// filled-region rule.
#[test]
fn block_has_filled_content_is_false_for_full_fill() {
    // -- Setup & Fixtures
    let user_block = Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        },
        lane: Lane::Marker("▏"),
        fill: Fill::Full,
        status: None,
    };

    // -- Exec & Check
    assert!(
        !user_block.has_filled_content(),
        "Fill::Full is not the code fill → false"
    );
}

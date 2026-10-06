use crate::state::{AppState, ToolCallLine, ToolCallStatus};
use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{
    Block, BlockSource, ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use nu_agent_core::transcript::items::{Message, Tool};
use nu_agent_core::transcript::renderer::{FrameContext, ItemStatus, Renderable};

use crate::rendering::theme::TuiTheme;

/// Construct a Block from a Renderable item in one expression — the direct
/// construction form the spec mandates for all callers.
fn block_from_item(item: &impl Renderable) -> Block {
    Block {
        source: item.source(),
        lane: item.lane(),
        fill: item.fill(),
        status: None,
    }
}

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The block cap that will be defined in production code.
/// Tests reference this to avoid magic numbers.
const MAX_TRANSCRIPT_BLOCKS: usize = 2000;

// ---------------------------------------------------------------------------
// Cap enforcement (migrated tests live with the spacer-rule tests below)
// ---------------------------------------------------------------------------

/// Every block the store holds is a content block — the store never inserts
/// Spacer blocks (separator rows are a render-time concern). Counts every
/// block; a store that inserted separators would fail this count.
fn content_block_count(state: &AppState) -> usize {
    state.transcript.blocks().len()
}

#[test]
fn transcript_cap_empty_transcript_no_panic() {
    let mut state = AppState::default();

    // RED: This will pass even without the cap (no-op on empty)
    state.transcript.enforce_transcript_cap();

    assert!(state.transcript.blocks().is_empty());
}

// ---------------------------------------------------------------------------
// Cap enforcement with mixed roles
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_enforced_with_alternating_turn_roles() {
    let mut state = AppState::default();

    // Push alternating turn roles (User/Assistant blocks with spacers between)
    for i in 0..MAX_TRANSCRIPT_BLOCKS / 2 {
        push_user(&mut state, &format!("user {i}"));
        push_assistant(&mut state, &format!("assistant {i}"));
    }

    // Push one more to exceed cap
    push_user(&mut state, "overflow");

    // Over-cap blocks were evicted; total stays at the cap
    assert_eq!(state.transcript.len(), MAX_TRANSCRIPT_BLOCKS);
}

// ---------------------------------------------------------------------------
// Index shifting — streaming_message_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_streaming_message_start_shifted() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(2010);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.transcript.assistant_stream_start, Some(10));
}

#[test]
fn shift_indices_streaming_message_start_evicted() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(5);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.transcript.assistant_stream_start, None);
}

#[test]
fn shift_indices_streaming_message_start_none_stays_none() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = None;

    state.transcript.shift_indices_after_eviction(2000);

    // RED: This will pass even with the stub (None stays None)
    assert_eq!(state.transcript.assistant_stream_start, None);
}

// ---------------------------------------------------------------------------
// Index shifting — compaction_streaming_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_compaction_streaming_start_shifted() {
    let mut state = AppState::default();
    state.transcript.summary_stream_start = Some(2010);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.transcript.summary_stream_start, Some(10));
}

#[test]
fn shift_indices_compaction_streaming_start_evicted() {
    let mut state = AppState::default();
    state.transcript.summary_stream_start = Some(5);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.transcript.summary_stream_start, None);
}

fn push_user(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::User,
        markdown: text.to_string(),
    };
    state.transcript.push_block(block_from_item(&msg));
}

fn push_assistant(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    };
    state.transcript.push_block(block_from_item(&msg));
}

// ---------------------------------------------------------------------------
// Store holds content blocks only — separators are a render-time concern
// (task 4609782e). The store never inserts Spacer blocks; the number of
// visual separator rows between two blocks is decided by the
// SpacerStateMachine during rendering (see spacer_test.rs).
// ---------------------------------------------------------------------------

#[test]
fn push_block_no_spacer_before_first_block() {
    let mut state = AppState::default();

    push_user(&mut state, "prompt one");

    assert_eq!(state.transcript.len(), 1);
    assert!(!matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Spacer
    ));
}

#[test]
fn push_block_two_user_blocks_store_two_content_blocks() -> Result<()> {
    let mut state = AppState::default();

    push_user(&mut state, "prompt one");
    push_user(&mut state, "prompt two");

    // [User, User] — the store holds two content blocks, no Spacer blocks.
    assert_eq!(state.transcript.len(), 2);
    assert!(!matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Spacer
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    ));
    Ok(())
}

#[test]
fn push_block_user_then_assistant_store_two_content_blocks() -> Result<()> {
    let mut state = AppState::default();

    push_user(&mut state, "prompt one");
    push_assistant(&mut state, "response one");

    // [User, Assistant] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));
    Ok(())
}

#[test]
fn push_block_assistant_then_user_store_two_content_blocks() -> Result<()> {
    let mut state = AppState::default();

    push_assistant(&mut state, "response one");
    push_user(&mut state, "prompt two");

    // [Assistant, User] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));
    Ok(())
}

#[test]
fn push_block_tool_then_assistant_store_two_content_blocks() -> Result<()> {
    let mut state = AppState::default();

    state.transcript.push_block(simple_tool_block());
    push_assistant(&mut state, "response one");

    // [Tool, Assistant] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));
    Ok(())
}

#[test]
fn push_block_three_user_and_assistant_blocks_store_three_content_blocks() -> Result<()> {
    let mut state = AppState::default();

    push_user(&mut state, "prompt one");
    push_assistant(&mut state, "response one");
    push_user(&mut state, "prompt two");

    // [User, Assistant, User] — three content blocks, no Spacer blocks.
    assert_eq!(state.transcript.len(), 3);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Markdown { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Markdown { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[2].source,
        BlockSource::Markdown { .. }
    ));
    Ok(())
}

#[test]
fn push_block_keeps_user_fill_and_lane() -> Result<()> {
    let mut state = AppState::default();

    let msg = Message {
        role: MessageRole::User,
        markdown: "hello".to_string(),
    };
    state.transcript.push_block(block_from_item(&msg));

    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have block")?;
    assert_eq!(block.lane, nu_agent_core::transcript::ir::Lane::Marker("▏"));
    assert_eq!(block.fill, nu_agent_core::transcript::ir::Fill::Full);
    Ok(())
}

#[test]
fn push_block_carries_tool_status() -> Result<()> {
    let mut state = AppState::default();

    let tool = nu_agent_core::transcript::items::Tool {
        name: ToolName("read".to_string()),
        call: CallLine::from_json_summary("{}"),
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    };
    state.transcript.push_block(Block {
        source: tool.source(),
        lane: tool.lane(),
        fill: tool.fill(),
        status: Some(ItemStatus::InProgress),
    });

    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have block")?;
    assert!(matches!(block.source, BlockSource::Tool { .. }));
    assert_eq!(block.status, Some(ItemStatus::InProgress));
    Ok(())
}

#[test]
fn push_block_notices_and_tool_display_store_content_only() -> Result<()> {
    let mut state = AppState::default();

    let notice = nu_agent_core::transcript::items::Notice {
        kind: NoticeKind::Compaction,
        text: "Compaction".to_string(),
    };
    state.transcript.push_block(block_from_item(&notice));
    state.transcript.push_block(Block {
        source: BlockSource::ToolDisplay { lines: Vec::new() },
        lane: nu_agent_core::transcript::ir::Lane::Blank,
        fill: nu_agent_core::transcript::ir::Fill::None,
        status: None,
    });

    // [Notice, ToolDisplay] — two content blocks, no Spacer blocks.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::ToolDisplay { .. }
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Cap enforcement
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_evicts_oldest_when_exceeded() {
    let mut state = AppState::default();

    // Push one more than the cap
    for i in 0..=MAX_TRANSCRIPT_BLOCKS {
        push_user(&mut state, &format!("entry {i}"));
    }

    // RED: This will fail because transcript_preview grows unbounded
    assert_eq!(state.transcript.len(), MAX_TRANSCRIPT_BLOCKS);

    // The oldest entry should have been evicted
    let first = state
        .transcript
        .blocks()
        .first()
        .expect("should have block");
    assert_ne!(
        first.source.plain_text(),
        "entry 0",
        "oldest entry should be evicted"
    );
}

#[test]
fn transcript_cap_no_eviction_below_cap() {
    let mut state = AppState::default();

    // One user block per push; the store no longer adds separator blocks, so
    // keep the total below the cap so nothing is evicted.
    let push_count = MAX_TRANSCRIPT_BLOCKS / 2;
    for i in 0..push_count {
        push_user(&mut state, &format!("entry {i}"));
    }

    // Nothing evicted: the store holds exactly the pushed content blocks.
    assert_eq!(state.transcript.len(), push_count);
    assert_eq!(content_block_count(&state), push_count);

    // All entries are present, in order.
    let content_blocks = state.transcript.blocks();
    assert_eq!(
        content_blocks[0].source.plain_text(),
        "entry 0",
        "first entry should still be present"
    );
    assert_eq!(
        content_blocks[push_count - 1].source.plain_text(),
        format!("entry {}", push_count - 1),
        "last entry should still be present"
    );
}

#[test]
fn markdown_projection_is_deterministic_for_same_input() {
    let markdown = "```rust\nfn main() {\n    let x = 42;\n}\n```";

    let first = crate::markdown::render_markdown_lines(markdown, None);
    let second = crate::markdown::render_markdown_lines(markdown, None);

    assert_eq!(first, second);
}

#[test]
fn push_transcript_item_follows_tail_when_at_last_item() {
    let mut state = AppState::default();

    // Push first item — following_tail starts true, stays true
    push_user(&mut state, "first");
    assert!(state.scroll.following_tail);

    // Push second item — should still follow
    push_assistant(&mut state, "second");
    assert!(state.scroll.following_tail);

    // Push third item — should still follow
    push_user(&mut state, "third");
    assert!(state.scroll.following_tail);
}

#[test]
fn push_transcript_item_stays_put_when_scrolled_up() {
    let mut state = AppState::default();

    // Push some items
    push_user(&mut state, "first");
    push_assistant(&mut state, "second");
    push_user(&mut state, "third");

    // Scroll to top (user has scrolled up — disables following)
    state.scroll.scroll_transcript_to_top();
    assert!(!state.scroll.following_tail);
    assert_eq!(state.scroll.scroll_offset, 0);

    // Push new item — should NOT re-enable following, offset stays at 0
    push_assistant(&mut state, "fourth");
    assert!(
        !state.scroll.following_tail,
        "following_tail should stay false when user has scrolled up"
    );
    assert_eq!(
        state.scroll.scroll_offset, 0,
        "scroll offset should stay at top when user has scrolled up"
    );
}

#[test]
fn push_transcript_item_follows_when_nothing_selected() {
    let mut state = AppState::default();

    // Initially following_tail is true (default)
    assert!(state.scroll.following_tail);

    // Push first item — following_tail stays true
    push_user(&mut state, "first");
    assert!(
        state.scroll.following_tail,
        "first push should keep following_tail true"
    );
}

#[test]
fn markdown_projection_ignores_cache_clearing() {
    let markdown = "hello world";

    let first = crate::markdown::render_markdown_lines(markdown, None);

    // There is no projection cache to clear anymore — projecting again must
    // produce the identical output.
    let second = crate::markdown::render_markdown_lines(markdown, None);

    assert_eq!(first, second, "projection must not depend on a cache");
}

#[test]
fn push_transcript_line_user_bold_markdown_emits_md_bold_span() -> Result<()> {
    let mut state = AppState::default();
    push_user(&mut state, "hello **world**");
    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    let BlockSource::Markdown { markdown, .. } = &last.source else {
        panic!("expected User markdown block");
    };
    // Raw markdown is stored; verify it projects to MdBold at render time
    let bold = crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");
    Ok(())
}

#[test]
fn push_transcript_line_assistant_bold_markdown_emits_md_bold_span() -> Result<()> {
    let mut state = AppState::default();
    push_assistant(&mut state, "hello **world**");
    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    let BlockSource::Markdown { markdown, .. } = &last.source else {
        panic!("expected Assistant markdown block");
    };
    let bold = crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");
    Ok(())
}

#[test]
fn push_transcript_line_user_and_assistant_produce_identical_lines_for_same_text() -> Result<()> {
    let mut s1 = AppState::default();
    let mut s2 = AppState::default();
    let text = "**bold** and *italic* and `code`".to_string();
    push_user(&mut s1, &text);
    push_assistant(&mut s2, &text);
    let last1 = s1
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    let last2 = s2
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    let BlockSource::Markdown { markdown: u, .. } = &last1.source else {
        panic!();
    };
    let BlockSource::Markdown { markdown: a, .. } = &last2.source else {
        panic!();
    };
    assert_eq!(u, a, "user and assistant prose must be byte-identical");
    Ok(())
}

#[test]
fn push_transcript_line_user_fenced_code_block_produces_multiple_lines() -> Result<()> {
    let mut state = AppState::default();
    push_user(&mut state, "```rust\nfn a() {}\nfn b() {}\n```");
    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    let BlockSource::Markdown { markdown, .. } = &last.source else {
        panic!("expected User markdown block");
    };
    // Verify projection of the stored raw markdown yields multiple lines
    let projected = crate::markdown::render_markdown_lines(markdown, None);
    assert!(projected.len() >= 2);
    Ok(())
}

// ---------------------------------------------------------------------------
// Height index — measure()-based (task 53012ecc)
// ---------------------------------------------------------------------------

/// Row count layout() produces for a user prose block at `width`.
fn rendered_row_count(markdown: &str, width: usize) -> usize {
    use nu_agent_core::transcript::ir::Fill;

    let msg = Message {
        role: MessageRole::User,
        markdown: markdown.to_string(),
    };
    let block = nu_agent_core::transcript::ir::Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: Fill::Full,
        status: None,
    };
    let ctx = FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    crate::tui_renderer::layout(&block, &ctx, &TuiTheme::default()).len()
}

#[test]
fn rebuild_height_index_total_rows_includes_separator_rows() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    push_user(&mut state, "short line");
    push_assistant(&mut state, "reply");
    let width = 80;

    // -- Exec
    state.transcript.rebuild_height_index(width);

    // -- Check: total = measure(block) for each block + the separator rows the
    // SpacerStateMachine decides between adjacent blocks. Two prose blocks
    // with no separator before the first: 0 + 2 = 2 separator rows.
    let measured: usize = state
        .transcript
        .blocks()
        .iter()
        .map(|block| crate::tui_renderer::measure(block, width, &TuiTheme::default()))
        .sum();
    let expected = measured + 2;
    assert_eq!(
        state.transcript.total_visual_rows(),
        expected,
        "total_rows must equal measure sum plus SM separator rows"
    );
    Ok(())
}

#[test]
fn visible_window_returns_block_range_covering_viewport() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    // Five user blocks. Each block after the first carries 2 separator rows
    // (user → user transition) plus its own 1 content row:
    //   block 0: rows 0        (start 0)
    //   block 1: rows 1..=3    (start 1)
    //   block 2: rows 4..=6    (start 4)
    //   block 3: rows 7..=9    (start 7)
    //   block 4: rows 10..=12  (start 10)
    for i in 0..5 {
        push_user(&mut state, &format!("line {i}"));
    }
    let width = 80;

    // -- Exec
    state.transcript.rebuild_height_index(width);

    // -- Check
    // Viewport rows 0..2: block 0 starts at 0 and block 1 starts at 1, so
    // both blocks intersect the window.
    let (first, last) = state.transcript.visible_window(0, 2);
    assert_eq!(first, 0);
    assert_eq!(last, 2);

    // Viewport rows 2..4 starts inside block 1 (rows 1..=3): only block 1.
    let (first, last) = state.transcript.visible_window(2, 2);
    assert_eq!(first, 1, "offset 2 is inside block 1");
    assert_eq!(last, 2, "block 2 starts at row 4, past the window end");
    Ok(())
}

#[test]
fn height_index_matches_layout_row_count_for_wrapping_prose() -> Result<()> {
    // A single long prose line that wraps at width 24 (20 chars available
    // after the 4-col lane prefix): 60 words of 3 chars => 240 chars total.
    let mut state = AppState::default();
    let long_line = vec!["abc"; 60].join(" ");
    push_user(&mut state, &long_line);

    for width in [24usize, 40, 80, 120] {
        state.transcript.rebuild_height_index(width);

        // The single user block is the final block, so the index adds one
        // trailing separator row on top of the layout row count (task
        // 670e0292).
        let expected = rendered_row_count(&long_line, width) + 1;
        assert_eq!(
            state.transcript.total_visual_rows(),
            expected,
            "height index must match rendered rows plus the trailing separator at width {width}"
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Hydration reconstructs Tool blocks (task 53012ecc)
// ---------------------------------------------------------------------------

#[test]
fn hydration_reconstructs_one_tool_block_with_call_and_status() -> Result<()> {
    let mut state = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();

    state.transcript.hydrate_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[k8s__list_pods] → {} · done").with_tool_details(
                Some(r#"{"namespace":"prod"}"#.to_string()),
                Some("[]".to_string()),
                Some(true),
            ),
        ],
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // One Tool block — not a start block + separate finish/notice entries.
    assert_eq!(state.transcript.len(), 1);
    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { call, .. } = &block.source else {
        panic!("expected Tool block");
    };
    // MCP names are not builtins, so the call line is the generic JSON
    // summary — the tool identity lives in the bookkeeping, not the line.
    assert!(call.summary.contains("namespace"));
    assert_eq!(
        block.status,
        Some(nu_agent_core::transcript::renderer::ItemStatus::Done)
    );
    Ok(())
}

#[test]
fn hydration_attaches_tool_display_as_preview_on_tool_block() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();

    // The resolver emits a persisted result display as a STANDALONE snapshot
    // — role "tool_display", empty content, no tool fields (resolver.rs
    // `hydrate_single_message`, UserContent::ToolResult branch). The tool
    // call snapshot from the Assistant ToolCall branch precedes it.
    let snapshots = vec![
        UiMessageSnapshot::new("tool", "→ {\"path\":\"a.rs\"}")
            .with_tool_name("edit".to_string())
            .with_tool_details(Some(r#"{"path":"a.rs"}"#.to_string()), None, Some(true)),
        UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(ToolDisplay {
            title: "edit a.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    ];

    // -- Exec
    state.transcript.hydrate_from_messages(
        snapshots,
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // -- Check
    // Two blocks: the Tool block (terminal status, no preview, no fill) and
    // the ToolDisplay block carrying the persisted display with Fill::Code.
    assert_eq!(state.transcript.len(), 2, "preview must add one block");
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    assert_eq!(
        tool_block.status,
        Some(ItemStatus::Done),
        "terminal status from the tool snapshot must survive"
    );
    assert_eq!(tool_block.fill, nu_agent_core::transcript::ir::Fill::None);

    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the persisted display must hydrate as its own ToolDisplay block"
    );
    assert_eq!(
        preview_block.fill,
        nu_agent_core::transcript::ir::Fill::Code
    );
    assert!(
        preview_block.source.plain_text().contains("+++ b"),
        "the preview block must carry the projected diff content, got: {}",
        preview_block.source.plain_text()
    );
    Ok(())
}

/// WHEN `hydrate_from_messages` receives one role-"user" snapshot with
/// multi-line content, THE TranscriptStore SHALL hold exactly one User block
/// whose rail is contiguous on every rendered row.
#[test]
fn hydration_user_multi_line_message_hydrates_one_block_with_contiguous_rail() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();

    // -- Exec
    state.transcript.hydrate_from_messages(
        vec![UiMessageSnapshot::new("user", "line1\n\nline2")],
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // -- Check
    assert_eq!(
        state.transcript.len(),
        1,
        "a multi-line user message must hydrate as one block"
    );
    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have user block")?;
    assert!(
        matches!(
            block.source,
            BlockSource::Markdown {
                role: MessageRole::User,
                ..
            }
        ),
        "expected one User markdown block, got {:?}",
        block.source
    );
    assert_eq!(block.lane, nu_agent_core::transcript::ir::Lane::Marker("▏"));
    assert_eq!(block.fill, nu_agent_core::transcript::ir::Fill::Full);
    assert_eq!(block.source.plain_text(), "line1\n\nline2");

    let ctx = FrameContext {
        width: 80,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    let lines = crate::tui_renderer::layout(block, &ctx, &TuiTheme::default());
    assert!(
        lines.len() >= 2,
        "fixture must render more than one row; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = line.spans.get(1).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Eviction shifts domain block_index bookkeeping (task 7f67b85b)
// ---------------------------------------------------------------------------

/// The shift method must move every surviving tool-call `block_index` down
/// by the evicted count.
#[test]
fn shift_bookkeeping_after_eviction_shifts_tool_call_block_index() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.tool.calls.push(ToolCallLine {
        id: 1,
        status: ToolCallStatus::InProgress,
        key: "edit\n{}".to_string(),
        block_index: Some(5),
    });

    // -- Exec
    state.shift_bookkeeping_after_eviction(3);

    // -- Check
    let call = state.tool.calls.first().ok_or("should have call")?;
    assert_eq!(call.block_index, Some(2), "5 - 3 = 2");
    Ok(())
}

#[test]
fn shift_bookkeeping_after_eviction_drops_tool_call_block_index_below_eviction() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.tool.calls.push(ToolCallLine {
        id: 1,
        status: ToolCallStatus::InProgress,
        key: "edit\n{}".to_string(),
        block_index: Some(2),
    });

    // -- Exec
    state.shift_bookkeeping_after_eviction(3);

    // -- Check
    let call = state.tool.calls.first().ok_or("should have call")?;
    assert_eq!(
        call.block_index, None,
        "block 2 was evicted (evicted 3) — index must be None"
    );
    Ok(())
}

#[test]
fn shift_bookkeeping_after_eviction_zero_count_is_noop() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.tool.calls.push(ToolCallLine {
        id: 1,
        status: ToolCallStatus::InProgress,
        key: "edit\n{}".to_string(),
        block_index: Some(5),
    });

    // -- Exec
    state.shift_bookkeeping_after_eviction(0);

    // -- Check
    let call = state.tool.calls.first().ok_or("should have call")?;
    assert_eq!(call.block_index, Some(5));
    Ok(())
}

#[test]
fn push_block_returns_zero_without_eviction() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check
    assert_eq!(state.transcript.push_block(simple_tool_block()), 0);
    assert_eq!(state.transcript.push_block(simple_tool_block()), 0);
    Ok(())
}

#[test]
fn push_block_returns_evicted_count_on_overflow() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    // The store holds content blocks only (no auto-inserted separators), so
    // each push adds exactly one block. Filling to the cap, then one more
    // push: the cap evicts exactly one block (the oldest).
    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    let evicted = state.transcript.push_block(simple_tool_block());

    // -- Check
    assert_eq!(
        state.transcript.len(),
        MAX_TRANSCRIPT_BLOCKS,
        "cap must stay enforced"
    );
    assert_eq!(evicted, 1, "the oldest block was evicted");
    Ok(())
}

#[test]
fn enforce_transcript_cap_on_empty_transcript_returns_zero() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check
    assert_eq!(state.transcript.enforce_transcript_cap(), 0);
    Ok(())
}

/// END-TO-END: eviction must keep `finish_tool_call` landing on the CORRECT
/// block. Start a tool call near the cap boundary, evict blocks in front of
/// it, then finish — the block AT THE SHIFTED INDEX must get the terminal
/// status, not some unrelated block.
#[test]
fn eviction_then_finish_tool_call_updates_block_at_shifted_index() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // Push to exactly the cap so the next tool-start evicts.
    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    // Start a tool call: one block pushed, one evicted (the oldest); the tool
    // block lands at the final index (cap - 1).
    let mut start_evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "read",
        "{}",
        CallLine::from_json_summary("{}"),
        &mut start_evicted,
    );
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index, MAX_TRANSCRIPT_BLOCKS,
        "fresh bookkeeping index is pre-compensated: store len 2000 minus 1, plus the 1 block evicted by this push"
    );
    // The dispatch seam shifts after every event in production; the test
    // replicates it because it drives the low-level methods. This shift
    // cancels the +1 pre-compensation above.
    state.shift_bookkeeping_after_eviction(start_evicted);
    assert_eq!(
        state
            .tool
            .calls
            .last()
            .ok_or("should have recorded call")?
            .block_index,
        Some(MAX_TRANSCRIPT_BLOCKS - 1),
        "after the seam shift the index is the tool block's true position"
    );

    // Push enough assistant blocks after it to evict the tool block itself.
    // Each push adds one block; each overflow evicts one.
    let assistant = block_from_item(&Message {
        role: MessageRole::Assistant,
        markdown: "filler".to_string(),
    });
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        let push_evicted = state.transcript.push_block(assistant.clone());
        total_evicted += push_evicted;
        // The seam shift after each event, mirroring production.
        state.shift_bookkeeping_after_eviction(push_evicted);
    }

    // -- Exec
    state
        .tool
        .finish_tool_call(&mut state.transcript, "read", "{}", Some(true));

    // -- Check
    // The bookkeeping entry must have been shifted to the tool block's new
    // position; the finish must have landed on THAT block.
    let call = state.tool.calls.last().ok_or("should have recorded call")?;
    // The per-push seam shifts already tracked every eviction, so the
    // bookkeeping index equals the tool block's true position: the record
    // time position (cap - 1) minus the total evicted count.
    let expected_index = (MAX_TRANSCRIPT_BLOCKS - 1)
        .checked_sub(total_evicted)
        .filter(|_| (MAX_TRANSCRIPT_BLOCKS - 1) >= total_evicted);
    assert_eq!(
        call.block_index, expected_index,
        "bookkeeping index must be shifted by the evicted count"
    );
    if let Some(expected_index) = expected_index {
        let block = state
            .transcript
            .blocks()
            .get(expected_index)
            .ok_or("expected tool block must exist")?;
        assert!(
            matches!(block.source, BlockSource::Tool { .. }),
            "shifted index must still point at the tool block"
        );
        assert_eq!(
            block.status,
            Some(ItemStatus::Done),
            "finish must land on the shifted-index tool block"
        );
    }
    Ok(())
}

/// END-TO-END for the preview path: after eviction, `set_tool_preview` must
/// push the preview block directly after the Tool block at the SHIFTED index,
/// not at a stale one.
#[test]
fn eviction_then_set_tool_preview_pushes_preview_after_shifted_tool_block() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    let mut start_evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut start_evicted,
    );
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index, MAX_TRANSCRIPT_BLOCKS,
        "fresh bookkeeping index is pre-compensated: store len 2000 minus 1, plus the 1 block evicted by this push"
    );
    // The dispatch seam shifts after every event in production; the test
    // replicates it because it drives the low-level methods. This shift
    // cancels the +1 pre-compensation above.
    state.shift_bookkeeping_after_eviction(start_evicted);

    // Evict blocks in front of the tool block with assistant pushes (each
    // adds one block and evicts one overflow block). The tool block survives
    // and its index shifts down by the total evicted count.
    let filler = block_from_item(&Message {
        role: MessageRole::Assistant,
        markdown: "filler".to_string(),
    });
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        let push_evicted = state.transcript.push_block(filler.clone());
        total_evicted += push_evicted;
        // The seam shift after each event, mirroring production.
        state.shift_bookkeeping_after_eviction(push_evicted);
    }
    assert!(total_evicted > 0, "eviction must have occurred");

    // -- Exec
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "changes".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );
    state.shift_bookkeeping_after_eviction(preview_evicted);

    // -- Check
    // The per-push seam shifts already tracked every eviction, so the
    // bookkeeping index equals the tool block's true position: the record
    // time position (cap - 1) minus the total evicted count, minus the one
    // block the preview insert itself evicted (the store was already at cap).
    let shifted_index = (MAX_TRANSCRIPT_BLOCKS - 1) - total_evicted - preview_evicted;
    let block = state
        .transcript
        .blocks()
        .get(shifted_index)
        .ok_or("shifted-index tool block must exist")?;
    assert!(
        matches!(block.source, BlockSource::Tool { .. }),
        "the Tool block must stay at the shifted index"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(shifted_index + 1)
        .ok_or("preview block must follow the tool block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview block must land directly after the shifted-index tool block"
    );
    Ok(())
}

/// Minimal Tool block for cap-boundary tests: carries no meaningful content.
fn simple_tool_block() -> Block {
    let tool = Tool {
        name: ToolName("read".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    };
    Block {
        source: tool.source(),
        lane: tool.lane(),
        fill: tool.fill(),
        status: Some(ItemStatus::InProgress),
    }
}

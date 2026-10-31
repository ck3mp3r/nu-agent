use crate::RenderBackend;
use crate::plain::PlainBackend;
use crate::planner::{EntryFamily, LayoutPlanner};
use crate::theme::Theme;
use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_core::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use agnt_core::transcript::renderer::ItemStatus;

// ---------------------------------------------------------------------------
// Gap rules — the SpacerStateMachine's transition rules, table-driven
// ---------------------------------------------------------------------------

/// Every predecessor/incoming pair over the four-family set, with no component
/// padding on either side. The counts are the landed `SpacerStateMachine` rules
/// verbatim: tool→tool gets 0, tool→assistant and assistant→tool get 1, every
/// other transition gets 2. This table is the pre-padding total pin.
#[test]
fn test_planner_gap_between_four_family_table() {
    use EntryFamily::{Assistant, Banner, Notice, Tool, User};

    // -- Setup & Fixtures
    let cases = [
        (Tool, Tool, 0),
        (Tool, User, 2),
        (Tool, Assistant, 1),
        (Tool, Notice, 2),
        (Tool, Banner, 2),
        (User, Tool, 2),
        (User, User, 2),
        (User, Assistant, 2),
        (User, Notice, 2),
        (User, Banner, 2),
        (Assistant, Tool, 1),
        (Assistant, User, 2),
        (Assistant, Assistant, 2),
        (Assistant, Notice, 2),
        (Assistant, Banner, 2),
        (Notice, Tool, 2),
        (Notice, User, 2),
        (Notice, Assistant, 2),
        (Notice, Notice, 2),
        (Notice, Banner, 2),
        (Banner, Tool, 2),
        (Banner, User, 2),
        (Banner, Assistant, 2),
        (Banner, Notice, 2),
        (Banner, Banner, 2),
    ];

    // -- Exec & Check
    for (prev, incoming, expected) in cases {
        assert_eq!(
            LayoutPlanner::gap_between(prev, incoming, 0, 0),
            expected,
            "gap {prev:?} -> {incoming:?}"
        );
    }
}

/// THE reconciled accounting table: the planner gap is the landed base spacing
/// minus the adjacent component paddings (the predecessor's bottom row above,
/// the incoming entry's top row below), so padding plus gap always totals the
/// landed spacing. Only a tool entry whose display paints a surface carries a
/// padding row (its band closes with it); every other family is flush, and the
/// trailing rule is untouched.
#[test]
fn test_planner_padding_plus_gap_equals_the_landed_total() {
    use EntryFamily::{Assistant, Banner, Notice, Tool, User};

    // -- Setup & Fixtures: (prev, incoming, prev_bottom, next_top, gap, landed).
    let cases = [
        // A plain tool is flush: the pair table supplies the whole spacing.
        (Tool, Tool, 0, 0, 0, 0),
        (Tool, Assistant, 0, 0, 1, 1),
        (Tool, Notice, 0, 0, 2, 2),
        // A surface-carrying tool closes with one blank row.
        (Tool, Tool, 1, 0, 0, 1),
        (Tool, Assistant, 1, 0, 0, 1),
        (Tool, Notice, 1, 0, 1, 2),
        (Tool, User, 1, 0, 1, 2),
        (Assistant, Tool, 0, 0, 1, 1),
        (Assistant, Assistant, 0, 0, 2, 2),
        (Assistant, User, 0, 0, 2, 2),
        (Assistant, Notice, 0, 0, 2, 2),
        (Assistant, Banner, 0, 0, 2, 2),
        (User, Assistant, 0, 0, 2, 2),
        (User, User, 0, 0, 2, 2),
        (User, Tool, 0, 0, 2, 2),
        (Notice, Assistant, 0, 0, 2, 2),
        (Notice, Notice, 0, 0, 2, 2),
        (Notice, Banner, 0, 0, 2, 2),
        (Banner, Assistant, 0, 0, 2, 2),
        (Banner, Banner, 0, 0, 2, 2),
    ];

    // -- Exec & Check
    for (prev, incoming, prev_bottom, next_top, expect_gap, landed) in cases {
        let gap = LayoutPlanner::gap_between(prev, incoming, prev_bottom, next_top);
        assert_eq!(
            gap, expect_gap,
            "gap {prev:?} -> {incoming:?} with padding {prev_bottom} + {next_top}"
        );
        assert_eq!(
            gap + prev_bottom + next_top,
            landed,
            "padding plus gap must total the landed spacing for {prev:?} -> {incoming:?}"
        );
    }
}

/// The subtraction saturates: no padding sum can underflow the base spacing.
#[test]
fn test_planner_gap_saturates_when_padding_exceeds_the_base() {
    // -- Exec & Check: zero base, seven padding rows.
    assert_eq!(
        LayoutPlanner::gap_between(EntryFamily::Tool, EntryFamily::Tool, 3, 4),
        0,
        "a padding sum above the base gap saturates at zero"
    );
}

/// Trailing row counts after the last entry: a user turn closes with one row,
/// every other family ends flush.
#[test]
fn test_planner_trailing_gap_four_family_table() {
    // -- Setup & Fixtures
    let cases = [
        (EntryFamily::User, 1, "user turn closes with one row"),
        (EntryFamily::Assistant, 0, "assistant ends flush"),
        (EntryFamily::Tool, 0, "tool ends flush"),
        (EntryFamily::Notice, 0, "notice ends flush"),
        (EntryFamily::Banner, 0, "banner ends flush"),
    ];

    // -- Exec & Check
    for (last, expected, name) in cases {
        assert_eq!(LayoutPlanner::trailing_gap(last), expected, "{name}");
    }
}

// ---------------------------------------------------------------------------
// Family classification
// ---------------------------------------------------------------------------

/// Every entry variant classifies to the family the gap rules consume.
#[test]
fn test_planner_entry_family_classification_table() {
    // -- Setup & Fixtures
    let entry_cases = [
        (user_entry("hi"), EntryFamily::User),
        (assistant_entry("hi"), EntryFamily::Assistant),
        (tool_entry("→ a.rs", None), EntryFamily::Tool),
        (
            TranscriptEntry::Notice(NoticeEntry {
                kind: NoticeKind::System,
                text: "note".to_string(),
            }),
            EntryFamily::Notice,
        ),
        (
            TranscriptEntry::Banner(BannerEntry {
                text: "banner".to_string(),
            }),
            EntryFamily::Banner,
        ),
    ];

    // -- Exec & Check
    for (entry, expected) in entry_cases {
        assert_eq!(EntryFamily::of(&entry), expected, "{entry:?}");
    }
}

// ---------------------------------------------------------------------------
// FramePlan (seam 2)
// ---------------------------------------------------------------------------
/// The planner derives the same gaps the pair rules decide, gives each entry
/// its own padding rows inside its rect, places every entry at a contiguous row
/// rect, and totals the frame. The code-display tool entry is interior, so it
/// keeps its closing blank row; every other entry is flush.
#[test]
fn test_planner_plan_rects_and_gaps() -> Result<()> {
    // -- Setup & Fixtures
    let width = 40;
    let backend = PlainBackend::default();
    let entries = vec![
        user_entry("hi"),
        assistant_entry("reply"),
        tool_entry("→ a.rs", None),
        tool_entry("→ a2.rs", Some(code_display())),
        tool_entry("→ b.rs", None),
        TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text: "note".to_string(),
        }),
    ];
    let expected_gaps = [0, 2, 1, 0, 0, 2];
    let mut expected_padding = [(0usize, 0usize); 6];
    expected_padding[3] = (0, 1);

    // -- Exec
    let plan = LayoutPlanner::plan(&entries, width, &backend);

    // -- Check
    assert_eq!(plan.entries.len(), entries.len());
    let mut expected_row = 0;
    for ((planned, entry), (gap, (top, bottom))) in plan
        .entries
        .iter()
        .zip(entries.iter())
        .zip(expected_gaps.into_iter().zip(expected_padding))
    {
        let content_rows = backend.measure(entry, width);
        assert_eq!(
            planned.gap_before, gap,
            "gap before entry {:?}",
            planned.family
        );
        assert_eq!(planned.rect.width, width);
        assert_eq!(planned.rect.content_offset, top, "{entry:?} top padding");
        assert_eq!(
            planned.rect.rows,
            content_rows + top + bottom,
            "{entry:?} spans its content plus its realized padding"
        );
        expected_row += gap;
        assert_eq!(planned.rect.row, expected_row);
        expected_row += planned.rect.rows;
    }
    assert_eq!(plan.trailing_gap, 0);
    assert_eq!(plan.total_rows, expected_row);
    Ok(())
}

/// The frame has no outer margin: a padding row with no neighbour to space
/// against is clipped. A lone code-display tool entry is exactly its content
/// rows, and a leading one keeps its closing blank row.
#[test]
fn test_planner_clips_padding_at_the_frame_edges() -> Result<()> {
    // -- Setup & Fixtures
    let backend = PlainBackend::default();
    let lone = vec![tool_entry("→ a.rs", Some(code_display()))];
    let pair = vec![
        tool_entry("→ a.rs", Some(code_display())),
        tool_entry("→ b.rs", None),
    ];

    // -- Exec
    let lone_plan = LayoutPlanner::plan(&lone, 40, &backend);
    let pair_plan = LayoutPlanner::plan(&pair, 40, &backend);
    let lone_rows = backend.measure(lone.first().ok_or("the lone entry")?, 40);

    // -- Check: the lone entry loses its padding row at the frame edge.
    let only = lone_plan.entries.first().ok_or("the plan has one entry")?;
    assert_eq!(only.rect.content_offset, 0, "no top margin before entry 0");
    assert_eq!(
        only.rect.rows, lone_rows,
        "no closing row after the last entry"
    );
    assert_eq!(lone_plan.total_rows, lone_rows);

    // -- Check: the leading entry keeps its closing row, and the pair totals it
    // plus the flush tool→tool spacing.
    let first = pair_plan.entries.first().ok_or("two planned entries")?;
    let second = pair_plan.entries.get(1).ok_or("two planned entries")?;
    assert_eq!(first.rect.content_offset, 0);
    assert_eq!(
        first.rect.rows,
        lone_rows + 1,
        "content plus the kept padding"
    );
    assert_eq!(first.gap_before, 0);
    assert_eq!(second.gap_before, 0, "the padding owns the whole spacing");
    assert_eq!(second.rect.content_offset, 0);
    assert_eq!(pair_plan.total_rows, first.rect.rows + second.rect.rows);
    Ok(())
}

/// The rect carries the entry's padding rows and the backend writes them: a
/// painted plan totals the planned row count, with the code-display tool's
/// closing row blank. A notice closes the frame, so no trailing gap row is owed.
#[test]
fn test_planner_padded_rects_paint_to_the_planned_rows() -> Result<()> {
    // -- Setup & Fixtures: user, code-display tool, notice.
    let width = 40;
    let entries = vec![
        user_entry("hi"),
        tool_entry("→ a.rs", Some(code_display())),
        TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text: "note".to_string(),
        }),
    ];
    let backend = PlainBackend::default();
    let plan = LayoutPlanner::plan(&entries, width, &backend);
    let mut painting = PlainBackend::default();

    // -- Exec
    for planned in &plan.entries {
        let entry = entries.get(planned.index).ok_or("planned index in range")?;
        painting.render(entry, planned.rect, &Theme::default());
    }

    // -- Check: painted rows equal the plan's total, and the tool's rect closes
    // with a blank padding row below its content.
    assert_eq!(
        painting.line_count(),
        plan.total_rows,
        "the painted rows must equal the planned total"
    );
    let tool = plan.entries.get(1).ok_or("the tool is planned")?;
    assert_eq!(
        tool.rect.content_offset, 0,
        "no top padding on a tool entry"
    );
    let bottom = tool.rect.end().saturating_sub(1);
    assert_eq!(
        painting.output().get(bottom).map(String::as_str),
        Some(""),
        "the closing padding row is blank"
    );
    Ok(())
}

/// Same entries, same width, same plan — the planner is pure.
#[test]
fn test_planner_plan_is_pure() -> Result<()> {
    // -- Setup & Fixtures
    let backend = PlainBackend::default();
    let entries = vec![
        user_entry("hi"),
        tool_entry("→ a.rs", Some(code_display())),
        tool_entry("→ b.rs", None),
        assistant_entry("done"),
    ];

    // -- Exec
    let first = LayoutPlanner::plan(&entries, 32, &backend);
    let second = LayoutPlanner::plan(&entries, 32, &backend);

    // -- Check
    assert_eq!(first, second);
    Ok(())
}

/// An empty entry list plans to an empty frame.
#[test]
fn test_planner_plan_empty_entries() -> Result<()> {
    // -- Setup & Fixtures
    let backend = PlainBackend::default();

    // -- Exec
    let plan = LayoutPlanner::plan(&[], 40, &backend);

    // -- Check
    assert!(plan.entries.is_empty());
    assert_eq!(plan.trailing_gap, 0);
    assert_eq!(plan.total_rows, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn user_entry(markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: markdown.to_string(),
    })
}

fn assistant_entry(markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: markdown.to_string(),
    })
}

fn tool_entry(summary: &str, preview: Option<Display>) -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Generic,
        call: ToolCall::new(
            ToolName("read".to_string()),
            "",
            "{}",
            CallLine {
                summary: summary.to_string(),
            },
        ),
        preview,
        result: None,
        status: ItemStatus::Done,
    })
}

fn code_display() -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Code {
                language: "rust".to_string(),
            },
            content: "fn a() {}".to_string(),
            stats: None,
        }],
    }
}

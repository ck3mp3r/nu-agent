use crate::catalog::{
    CallLineRow, CatalogComponent, CodeSurface, DiffView, LaneKind, LaneLine, StatRow, SurfaceRow,
};
use agnt_core::protocol::event::ToolDisplayStats;
use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_core::transcript::ir::{ContentLine, DiffTint, MessageRole, NoticeKind, StyleHint};
use agnt_core::transcript::renderer::ItemStatus;

// ---------------------------------------------------------------------------
// LaneLine
// ---------------------------------------------------------------------------

/// Every entry family maps onto its lane: the user rail, the tool marker, the
/// notice markers, the blank prose lane, and the banner lane.
#[test]
fn test_catalog_lane_kind_of_entry_classifies_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (message_entry(MessageRole::User), LaneKind::User),
        (message_entry(MessageRole::Assistant), LaneKind::Assistant),
        (tool_entry(), LaneKind::Tool),
        (notice_entry(NoticeKind::Compaction), LaneKind::Compaction),
        (notice_entry(NoticeKind::System), LaneKind::System),
        (banner_entry(), LaneKind::Banner),
    ];

    // -- Exec & Check
    for (entry, expected) in cases {
        assert_eq!(LaneKind::of_entry(&entry), expected);
    }
    Ok(())
}

/// The lane prefix: the tool, notice, and banner lanes reserve a cursor slot
/// ahead of their marker; the user rail and prose lanes reserve the marker
/// slot alone.
#[test]
fn test_catalog_lane_line_prefix_widths_and_glyphs() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (LaneKind::User, "", 2),
        (LaneKind::Assistant, "", 2),
        (LaneKind::Tool, "  ", 4),
        (LaneKind::Compaction, "  ", 4),
        (LaneKind::System, "  ", 4),
        (LaneKind::Banner, "  ", 4),
    ];

    // -- Exec & Check
    for (kind, slot, width) in cases {
        let line = LaneLine::new(kind);
        assert_eq!(line.prefix_width(), width, "{kind:?} prefix width");
        assert_eq!(
            line.prefix(false),
            format!("{slot}{}", kind.marker()),
            "{kind:?} row-0 prefix"
        );
        assert_eq!(
            line.prefix(true),
            format!("{slot}{}", kind.continuation_marker()),
            "{kind:?} continuation prefix"
        );
    }
    Ok(())
}

/// The user rail stays on every row; the tool lane keeps only its cursor slot
/// on continuation rows.
#[test]
fn test_catalog_lane_line_user_rail_survives_continuation() -> Result<()> {
    // -- Setup & Fixtures
    let user = LaneLine::new(LaneKind::User);
    let tool = LaneLine::new(LaneKind::Tool);

    // -- Exec & Check
    assert_eq!(user.prefix(false), "╎ ");
    assert_eq!(user.prefix(true), "╎ ");
    assert_eq!(tool.prefix(false), "  ⚙ ");
    assert_eq!(tool.prefix(true), "    ");
    Ok(())
}

/// The cursor slot paints `> ` on the row it marks and blank everywhere else.
#[test]
fn test_catalog_lane_line_cursor_slot_marks_row_zero_only() -> Result<()> {
    // -- Setup & Fixtures
    let line = LaneLine::new(LaneKind::Tool).with_cursor();

    // -- Exec & Check
    assert_eq!(line.prefix(false), "> ⚙ ");
    assert_eq!(line.prefix(true), "    ", "continuations blank the cursor");
    assert_eq!(line.prefix_width(), 4, "the cursor never widens the lane");
    Ok(())
}

// ---------------------------------------------------------------------------
// CallLineRow
// ---------------------------------------------------------------------------

/// The call line is the tool name in emphasis plus the summary in muted, with
/// the omission rules the call-line projection uses.
#[test]
fn test_catalog_call_line_row_spans_compose_name_and_summary() -> Result<()> {
    // -- Setup & Fixtures
    let full = CallLineRow::new("read", "→ a.rs", None);
    let no_summary = CallLineRow::new("read", "", None);
    let no_name = CallLineRow::new("", "→ a.rs", None);

    // -- Exec & Check
    assert_eq!(
        hints(&full.spans()),
        [("read", StyleHint::Emphasis), (" → a.rs", StyleHint::Muted)]
    );
    assert_eq!(hints(&no_summary.spans()), [("read", StyleHint::Emphasis)]);
    assert_eq!(hints(&no_name.spans()), [("→ a.rs", StyleHint::Muted)]);
    Ok(())
}

/// The status indicator is the status type's own glyph; an unstatted call has
/// no indicator.
#[test]
fn test_catalog_call_line_row_indicator_follows_status() -> Result<()> {
    // -- Setup & Fixtures
    let done = CallLineRow::new("read", "→ a.rs", Some(ItemStatus::Done));
    let running = CallLineRow::new("read", "→ a.rs", Some(ItemStatus::InProgress));
    let statless = CallLineRow::new("read", "→ a.rs", None);

    // -- Exec & Check
    assert_eq!(done.indicator(0), Some("✓"));
    assert_eq!(running.indicator(0), Some("⠋"));
    assert_eq!(running.indicator(100), Some("⠙"));
    assert_eq!(statless.indicator(0), None);
    assert_eq!(done.plain_text(0), "✓ read → a.rs");
    assert_eq!(statless.plain_text(0), "read → a.rs");
    Ok(())
}

// ---------------------------------------------------------------------------
// StatRow
// ---------------------------------------------------------------------------

/// The stats row renders the display's own formatted stats line in muted, and
/// yields no row when the stats carry no value.
#[test]
fn test_catalog_stat_row_renders_formatted_stats() -> Result<()> {
    // -- Setup & Fixtures
    let stats = ToolDisplayStats {
        files_changed: Some(1),
        insertions: Some(2),
        deletions: Some(0),
        ..ToolDisplayStats::default()
    };
    let row = StatRow::new(stats);
    let empty = StatRow::new(ToolDisplayStats::default());

    // -- Exec & Check
    assert_eq!(row.text().as_deref(), Some("files=1 +2 -0"));
    assert_eq!(hints(&row.spans()), [("files=1 +2 -0", StyleHint::Muted)]);
    assert_eq!(empty.text(), None);
    assert!(empty.spans().is_empty());
    assert!(empty.is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// DiffView
// ---------------------------------------------------------------------------

/// A diff view paints each row's own tint; the line-number gutter travels in
/// the row's spans.
#[test]
fn test_catalog_diff_view_reports_row_tints() -> Result<()> {
    // -- Setup & Fixtures
    let view = DiffView::new(vec![
        ContentLine::single("@@ -1 +1 @@".to_string(), StyleHint::DiffHunk),
        ContentLine::single_with_tint(
            "-   1      │old".to_string(),
            StyleHint::Normal,
            DiffTint::Remove,
        ),
        ContentLine::single_with_tint(
            "+        1 │new".to_string(),
            StyleHint::Normal,
            DiffTint::Add,
        ),
    ]);

    // -- Exec & Check
    assert_eq!(view.len(), 3);
    assert_eq!(view.tint(0), None, "a hunk header carries no tint");
    assert_eq!(view.tint(1), Some(&DiffTint::Remove));
    assert_eq!(view.tint(2), Some(&DiffTint::Add));
    assert_eq!(view.tint(3), None, "past the end carries no tint");
    Ok(())
}

// ---------------------------------------------------------------------------
// CodeSurface
// ---------------------------------------------------------------------------

/// A code surface reserves one margin row above and below its rows.
#[test]
fn test_catalog_code_surface_reserves_margin_rows() -> Result<()> {
    // -- Setup & Fixtures
    let surface = CodeSurface::new(vec![
        SurfaceRow::Text(ContentLine::single(
            "fn a() {}".to_string(),
            StyleHint::Normal,
        )),
        SurfaceRow::Diff(DiffView::new(vec![ContentLine::single(
            "+        1 │new".to_string(),
            StyleHint::Normal,
        )])),
    ]);
    let empty = CodeSurface::new(Vec::new());

    // -- Exec & Check
    assert_eq!(CodeSurface::MARGIN_ROWS, 2);
    assert_eq!(surface.len(), 2, "two source rows");
    assert!(!surface.is_empty());
    assert!(empty.is_empty());
    Ok(())
}

/// The catalog bundle carries all five component kinds.
#[test]
fn test_catalog_component_bundles_every_kind() -> Result<()> {
    // -- Setup & Fixtures
    let components = [
        CatalogComponent::LaneLine(LaneLine::new(LaneKind::Tool)),
        CatalogComponent::CallLineRow(CallLineRow::new("read", "→ a.rs", None)),
        CatalogComponent::StatRow(StatRow::new(ToolDisplayStats::default())),
        CatalogComponent::DiffView(DiffView::new(Vec::new())),
        CatalogComponent::CodeSurface(CodeSurface::new(Vec::new())),
    ];

    // -- Exec & Check
    let names: Vec<&str> = components.iter().map(CatalogComponent::name).collect();
    assert_eq!(
        names,
        [
            "lane-line",
            "call-line-row",
            "stat-row",
            "diff-view",
            "code-surface"
        ]
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn hints(spans: &[agnt_core::transcript::ir::Span]) -> Vec<(&str, StyleHint)> {
    spans
        .iter()
        .map(|span| (span.text.as_str(), span.hint.clone()))
        .collect()
}

fn message_entry(role: MessageRole) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: "hi".to_string(),
    })
}

fn tool_entry() -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Generic,
        call: agnt_core::protocol::events::ToolCall::new(
            agnt_core::transcript::ir::ToolName("read".to_string()),
            "",
            "{}",
            agnt_core::protocol::tool_args::CallLine {
                summary: "→ a.rs".to_string(),
            },
        ),
        preview: None,
        result: None,
        status: ItemStatus::Done,
    })
}

fn notice_entry(kind: NoticeKind) -> TranscriptEntry {
    TranscriptEntry::Notice(NoticeEntry {
        kind,
        text: "notice".to_string(),
    })
}

fn banner_entry() -> TranscriptEntry {
    TranscriptEntry::Banner(BannerEntry {
        text: "banner".to_string(),
    })
}

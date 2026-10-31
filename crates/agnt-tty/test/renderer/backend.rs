//! TtyBackend tests: per-family decoration, the indicator clock, the color
//! gate, the measure/render contract, and wrap parity with the plain backend.

use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_core::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::{Gallery, PlainBackend, RenderBackend, RowRect, Theme};

use crate::renderer::backend::TtyBackend;

// ---------------------------------------------------------------------------
// Family decoration
// ---------------------------------------------------------------------------

/// The user turn carries the `[user] ` prefix on its first row; the assistant
/// turn carries none.
#[test]
fn test_renderer_backend_message_role_decides_the_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let user = message_entry(MessageRole::User, "hello");
    let assistant = message_entry(MessageRole::Assistant, "hi");

    // -- Exec
    let user_rows = render(&user, 40, false);
    let assistant_rows = render(&assistant, 40, false);

    // -- Check
    assert_eq!(user_rows, ["[user] hello"]);
    assert_eq!(assistant_rows, ["hi"]);
    Ok(())
}

/// A tool's first row is the `[tool] ` prefix, the status indicator, the name,
/// and the call summary; the preview content follows on its own rows.
#[test]
fn test_renderer_backend_tool_row_carries_prefix_indicator_and_call_line() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry("read", "→ a.rs", Some(plain_display("fn a() {}")));

    // -- Exec
    let rows = render(&entry, 40, false);

    // -- Check
    assert_eq!(rows, ["[tool] ✓ read → a.rs", "fn a() {}"]);
    Ok(())
}

/// The pre-refactor fallbacks survive: an empty name renders the summary alone,
/// and an empty summary leaves the name with no trailing space.
#[test]
fn test_renderer_backend_tool_name_and_summary_fallbacks() -> Result<()> {
    // -- Setup & Fixtures
    let nameless = tool_entry("", "→ a.rs", None);
    let summariless = tool_entry("read", "", None);

    // -- Exec
    let nameless_rows = render(&nameless, 40, false);
    let summariless_rows = render(&summariless, 40, false);

    // -- Check
    assert_eq!(nameless_rows, ["[tool] ✓ → a.rs"]);
    assert_eq!(summariless_rows, ["[tool] ✓ read"]);
    Ok(())
}

/// The notice prefix follows the notice kind; the banner carries none.
#[test]
fn test_renderer_backend_notice_and_banner_prefixes() -> Result<()> {
    // -- Setup & Fixtures
    let compaction = notice_entry(NoticeKind::Compaction, "summarized 12 turns");
    let system = notice_entry(NoticeKind::System, "transcript is short");
    let banner = TranscriptEntry::Banner(BannerEntry {
        text: "nu-agent".to_string(),
    });

    // -- Exec
    let compaction_rows = render(&compaction, 40, false);
    let system_rows = render(&system, 40, false);
    let banner_rows = render(&banner, 40, false);

    // -- Check
    assert_eq!(compaction_rows, ["[compaction] summarized 12 turns"]);
    assert_eq!(system_rows, ["[system] transcript is short"]);
    assert_eq!(banner_rows, ["nu-agent"]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Indicator clock
// ---------------------------------------------------------------------------

/// The in-progress indicator is the shared `ItemStatus` method's output and it
/// animates across 100 ms.
#[test]
fn test_renderer_backend_indicator_animates_with_the_clock() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry_with_status(ItemStatus::InProgress, "t", "→ t", None);

    // -- Exec
    let first = render_at(&entry, 0);
    let second = render_at(&entry, 100);

    // -- Check
    assert_eq!(first, ["[tool] ⠋ t → t"]);
    assert_eq!(
        second,
        [format!(
            "[tool] {} t → t",
            ItemStatus::InProgress.indicator_char(100)
        )]
    );
    assert_ne!(first, second, "the indicator must animate across 100 ms");
    Ok(())
}

// ---------------------------------------------------------------------------
// Color gate
// ---------------------------------------------------------------------------

/// With color off every gallery entry renders without an escape code.
#[test]
fn test_renderer_backend_color_off_emits_no_escape_codes() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;

    // -- Exec & Check
    for sample in Gallery::samples() {
        for row in render(&sample.entry, width, false) {
            assert!(
                !row.contains('\x1b'),
                "{} renders plain: {row:?}",
                sample.name
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Frame accounting
// ---------------------------------------------------------------------------

/// `measure` reports exactly the rows `render` writes, for every family.
#[test]
fn test_renderer_backend_measure_matches_rendered_rows() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;

    // -- Exec & Check
    for sample in Gallery::samples() {
        let backend = TtyBackend::new(true);
        let measured = backend.measure(&sample.entry, width);
        let mut painting = TtyBackend::new(true);
        painting.render(
            &sample.entry,
            RowRect::new(0, width, measured),
            &Theme::default(),
        );
        assert_eq!(
            painting.line_count(),
            measured,
            "measure and render must agree for {}",
            sample.name
        );
        assert!(measured > 0, "{} renders at least one row", sample.name);
    }
    Ok(())
}

/// A non-zero rect row pads the output so the entry lands on its planned row.
#[test]
fn test_renderer_backend_render_positions_at_rect_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "row three");
    let mut backend = TtyBackend::new(false);

    // -- Exec
    backend.render(&entry, RowRect::new(2, 40, 1), &Theme::default());

    // -- Check
    assert_eq!(backend.line_count(), 3);
    assert_eq!(backend.output(), ["", "", "[user] row three"]);
    assert_eq!(backend.to_text(), "\n\n[user] row three");

    // -- Exec: clear drops every row.
    backend.clear();

    // -- Check
    assert_eq!(backend.line_count(), 0);
    assert!(backend.output().is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Wrap parity
// ---------------------------------------------------------------------------

/// A content line wider than the rect breaks at the plain backend's points: the
/// TTY's rows, decoration stripped, are the plain backend's rows.
#[test]
fn test_renderer_backend_wrap_parity_with_plain_backend() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "alpha beta gamma delta");
    let width = 12;

    // -- Exec
    let plain = plain_rows(&entry, width);
    let rows = render(&entry, width, false);
    let first = rows.first().ok_or("first row")?;
    let mut stripped = vec![
        first
            .strip_prefix("[user] ")
            .ok_or("user prefix")?
            .to_string(),
    ];
    stripped.extend(rows.iter().skip(1).cloned());

    // -- Check
    assert_eq!(stripped, plain);
    assert_eq!(plain, ["alpha beta", "gamma delta"]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn render(entry: &TranscriptEntry, width: usize, use_color: bool) -> Vec<String> {
    let mut backend = TtyBackend::new(use_color);
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(0, width, rows), &Theme::default());
    backend.output().to_vec()
}

fn render_at(entry: &TranscriptEntry, now_millis: u128) -> Vec<String> {
    let mut backend = TtyBackend::new(false).with_now_millis(now_millis);
    let rows = backend.measure(entry, 40);
    backend.render(entry, RowRect::new(0, 40, rows), &Theme::default());
    backend.output().to_vec()
}

fn plain_rows(entry: &TranscriptEntry, width: usize) -> Vec<String> {
    let mut backend = PlainBackend::default();
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(0, width, rows), &Theme::default());
    backend.output().to_vec()
}

fn message_entry(role: MessageRole, markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: markdown.to_string(),
    })
}

fn tool_entry(name: &str, summary: &str, preview: Option<Display>) -> TranscriptEntry {
    tool_entry_with_status(ItemStatus::Done, name, summary, preview)
}

fn tool_entry_with_status(
    status: ItemStatus,
    name: &str,
    summary: &str,
    preview: Option<Display>,
) -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Generic,
        call: ToolCall::new(
            ToolName(name.to_string()),
            "",
            "{}",
            CallLine {
                summary: summary.to_string(),
            },
        ),
        preview,
        result: None,
        status,
    })
}

fn notice_entry(kind: NoticeKind, text: &str) -> TranscriptEntry {
    TranscriptEntry::Notice(NoticeEntry {
        kind,
        text: text.to_string(),
    })
}

fn plain_display(content: &str) -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Plain,
            content: content.to_string(),
            stats: None,
        }],
    }
}

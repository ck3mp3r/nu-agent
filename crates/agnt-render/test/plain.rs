use crate::RenderBackend;
use crate::backend::RowRect;
use crate::gallery::Gallery;
use crate::plain::PlainBackend;
use crate::theme::Theme;
use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{MessageEntry, ToolEntry, ToolKind, TranscriptEntry};
use agnt_core::transcript::ir::{ContentKind, Display, DisplaySection, MessageRole, ToolName};
use agnt_core::transcript::renderer::ItemStatus;

// ---------------------------------------------------------------------------
// Flat output
// ---------------------------------------------------------------------------

/// The plain backend writes the content text alone: no lane marker, no rail,
/// no leading pad.
#[test]
fn test_plain_backend_renders_flat_text() -> Result<()> {
    // -- Setup & Fixtures
    let entry = user_entry("Hello there");
    let rows = PlainBackend::default().measure(&entry, 40);
    let mut backend = PlainBackend::default();

    // -- Exec
    backend.render(&entry, RowRect::new(0, 40, rows), &Theme::default());

    // -- Check
    assert_eq!(backend.output(), ["Hello there"]);
    for line in backend.output() {
        assert!(
            !line.starts_with('⚙') && !line.starts_with('╎') && !line.starts_with('·'),
            "no lane marker in {line:?}"
        );
        assert!(!line.starts_with(' '), "no leading pad in {line:?}");
    }
    Ok(())
}

/// A tool entry is its call line plus the attached preview content — no
/// background margin rows above or below the code surface.
#[test]
fn test_plain_backend_tool_renders_call_line_and_preview() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry("→ a.rs", code_display());
    let rows = PlainBackend::default().measure(&entry, 40);
    let mut backend = PlainBackend::default();

    // -- Exec
    backend.render(&entry, RowRect::new(0, 40, rows), &Theme::default());

    // -- Check
    assert_eq!(backend.output(), ["read → a.rs", "fn a() {}"]);
    Ok(())
}

/// `measure` reports exactly the rows `render` writes, for every family.
#[test]
fn test_plain_backend_measure_matches_rendered_rows() -> Result<()> {
    // -- Setup & Fixtures
    let backend = PlainBackend::default();

    // -- Exec & Check
    for sample in Gallery::samples() {
        let width = Gallery::WIDTH;
        let measured = backend.measure(&sample.entry, width);
        let mut painting = PlainBackend::default();
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
fn test_plain_backend_render_positions_at_rect_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = user_entry("row three");
    let mut backend = PlainBackend::default();

    // -- Exec
    backend.render(&entry, RowRect::new(2, 40, 1), &Theme::default());

    // -- Check
    assert_eq!(backend.line_count(), 3);
    assert_eq!(backend.output(), ["", "", "row three"]);
    Ok(())
}

/// Long text wraps at the rect width; every row fits.
#[test]
fn test_plain_backend_wraps_to_width() -> Result<()> {
    // -- Setup & Fixtures
    let entry = user_entry("alpha beta gamma delta");
    let width = 12;
    let rows = PlainBackend::default().measure(&entry, width);
    let mut backend = PlainBackend::default();

    // -- Exec
    backend.render(&entry, RowRect::new(0, width, rows), &Theme::default());

    // -- Check
    assert_eq!(backend.output(), ["alpha beta", "gamma delta"]);
    for line in backend.output() {
        assert!(line.chars().count() <= width, "{line:?} fits {width}");
    }
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

fn tool_entry(summary: &str, preview: Display) -> TranscriptEntry {
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
        preview: Some(preview),
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

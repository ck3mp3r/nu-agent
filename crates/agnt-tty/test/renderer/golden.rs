//! The cross-backend golden: the TTY's content rows are the plain backend's
//! rows for the same entry, and the color gate only adds escapes.
//!
//! The gallery is the fixture source (design §3.4/§4.3). Two backends are
//! compared here: the plain baseline that every styled backend must carry, and
//! the TTY. The TUI's content parity is pinned by its own widget-level buffer
//! assertions, because `agnt-tui` does not depend on `agnt-tty` and `agnt-tty`
//! must not depend on `agnt-tui`.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{MessageRole, NoticeKind};
use agnt_render::{Gallery, PlainBackend, RenderBackend, RowRect, Theme};

use crate::renderer::backend::TtyBackend;

// ---------------------------------------------------------------------------
// Same field, same rows
// ---------------------------------------------------------------------------

/// Every gallery entry renders the same content rows through both backends —
/// the same field projection, once decorated for the terminal (criterion 1).
#[test]
fn test_golden_tty_rows_match_plain_rows() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;

    // -- Exec & Check
    for sample in Gallery::samples() {
        let plain = plain_rows(&sample.entry, width);
        let tty = tty_rows(&sample.entry, width, false);
        let stripped = strip_decoration(&sample.entry, &tty)?;
        assert_eq!(
            stripped, plain,
            "{} content rows must match the plain backend",
            sample.name
        );
    }
    Ok(())
}

/// The decorated rows are the golden fixtures: one exact rendering per family.
#[test]
fn test_golden_decorated_rows_per_family() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        ("message", vec!["[user] What changed in the render loop?"]),
        (
            "tool",
            vec!["[tool] ✓ read → src/render.rs", "seam 2 now plans the gaps"],
        ),
        (
            "tool-result",
            vec!["[tool] ✓ edit → src/render.rs", "result body"],
        ),
        (
            "notice",
            vec!["[system] Compaction skipped: transcript is short"],
        ),
        ("banner", vec!["nu-agent"]),
        (
            "tool-diff",
            vec![
                "[tool] ✓ edit → crates/agnt-render/src/plain.rs",
                "(diff)",
                "files=1 +2 -1",
                "--- a/crates/agnt-render/src/plain.rs",
                "+++ b/crates/agnt-render/src/plain.rs",
                "@@ -118,7 +118,8 @@",
                "  118  118 │fn content_rows(entry:",
                "&TranscriptEntry, width: usize) ->",
                "Vec<String> {",
                "  119  119 │    let width =",
                "width.max(1);",
                "- 120      │    content_lines(entry,",
                "width)",
                "+      120 │    let wrapping =",
                "WrapMode::first_fit(width);",
                "+      121 │    content_lines(entry,",
                "wrapping)",
                "  121  122 │        .iter()",
                "  122  123 │        .flat_map(|line|",
                "wrap_line(&rendered_line_to_plain_text(l",
                "ine), width))",
                "  123  124 │        .collect()",
                "}",
            ],
        ),
        (
            "message-blank-lines",
            vec![
                "Two seams moved in this wave.",
                "",
                "The planner now plans the gap rows, so a",
                "wrapped preview keeps its spacing.",
                "",
                "The TTY strips the escapes back to the",
                "plain rows.",
            ],
        ),
    ];

    // -- Exec
    let samples = Gallery::samples();

    // -- Check
    assert_eq!(samples.len(), expected.len(), "one sample per family");
    for (sample, (name, rows)) in samples.iter().zip(expected) {
        assert_eq!(sample.name, name);
        assert_eq!(
            tty_rows(&sample.entry, Gallery::WIDTH, false),
            rows,
            "{name} decorated rows"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Color gate
// ---------------------------------------------------------------------------

/// With color on, stripping the escapes returns the color-off rows, and escapes
/// appear exactly where a span carries styling (criterion 2).
#[test]
fn test_golden_ansi_gate_strips_to_the_color_off_rows() -> Result<()> {
    // -- Setup & Fixtures
    let width = Gallery::WIDTH;
    let mut styled_samples = 0usize;

    // -- Exec & Check
    for sample in Gallery::samples() {
        let plain_color = tty_rows(&sample.entry, width, false);
        let raw = tty_rows(&sample.entry, width, true);
        let stripped: Vec<String> = raw.iter().map(|row| strip_ansi(row)).collect();
        assert_eq!(
            stripped, plain_color,
            "{} ANSI-stripped rows must equal the color-off rows",
            sample.name
        );
        let has_escape = raw.iter().any(|row| row.contains('\x1b'));
        assert_eq!(
            has_escape,
            raw != plain_color,
            "{} escapes appear exactly where styling applies",
            sample.name
        );
        if has_escape {
            styled_samples += 1;
        }
    }
    assert!(
        styled_samples > 0,
        "at least one gallery sample must carry styled spans"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn tty_rows(entry: &TranscriptEntry, width: usize, use_color: bool) -> Vec<String> {
    let mut backend = TtyBackend::new(use_color);
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(0, width, rows), &Theme::default());
    backend.output().to_vec()
}

fn plain_rows(entry: &TranscriptEntry, width: usize) -> Vec<String> {
    let mut backend = PlainBackend::default();
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(0, width, rows), &Theme::default());
    backend.output().to_vec()
}

/// Drop the entry's own decoration from row 0: the family prefix and the tool
/// status indicator. What remains is content.
fn strip_decoration(entry: &TranscriptEntry, rows: &[String]) -> Result<Vec<String>> {
    let mut stripped = rows.to_vec();
    let Some(first) = stripped.first_mut() else {
        return Ok(stripped);
    };
    let content = match entry {
        TranscriptEntry::Message(message) => match message.role {
            MessageRole::User => strip_prefix(first, "[user] ")?,
            MessageRole::Assistant => first.clone(),
        },
        TranscriptEntry::Tool(tool) => {
            let call_line = strip_prefix(first, "[tool] ")?;
            let indicator = tool.status.indicator_char(0);
            strip_prefix(&call_line, &format!("{indicator} "))?
        }
        TranscriptEntry::Notice(notice) => match notice.kind {
            NoticeKind::Compaction => strip_prefix(first, "[compaction] ")?,
            NoticeKind::System => strip_prefix(first, "[system] ")?,
        },
        TranscriptEntry::Banner(_) => first.clone(),
    };
    *first = content;
    Ok(stripped)
}

fn strip_prefix(row: &str, prefix: &str) -> Result<String> {
    row.strip_prefix(prefix)
        .map(str::to_string)
        .ok_or_else(|| format!("row {row:?} must start with {prefix:?}").into())
}

/// Drop ANSI SGR sequences: an escape starts at `\x1b` and ends at the next
/// `m`.
fn strip_ansi(text: &str) -> String {
    let mut stripped = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            stripped.push(ch);
        }
    }
    stripped
}

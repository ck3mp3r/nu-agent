//! The backend's own text primitives: wrapping a projected content line into
//! visual rows and painting one row into the buffer.
//!
//! These belong to the TUI backend, not to a cross-backend helper layer: the
//! catalog components and the family widgets compose them so `measure` and
//! `paint` agree row for row by construction. Styling is resolved here from the
//! backend-neutral theme, so every widget paints through one vocabulary.

use agnt_core::transcript::ir::{ContentLine, DiffTint, Span, StyleHint};
use agnt_render::{Color, Theme};
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

// region:    --- Types

/// One visual row of a wrapped content line.
pub(crate) type VisualRow = Vec<Span>;

/// One row to paint: the lane gutter, an optional leading cell (the status
/// indicator), the content spans, and the row's full-width background.
pub(crate) struct RowPaint<'a> {
    pub gutter: Option<(&'a str, Style)>,
    pub leading: Option<(&'a str, Style)>,
    pub spans: &'a [Span],
    /// Base text style for `Normal` and `Emphasis` spans.
    pub base: Style,
    /// Full-width row background, when the row paints a surface.
    pub bg: Option<Color>,
}

// endregion: --- Types

// region:    --- Wrap

/// Wrap one content line into visual rows at `width` display columns. An empty
/// line still costs one row.
///
/// Single-span lines wrap one-to-one. Multi-span lines wrap on the joined
/// span text; the wrapper's rows are contiguous substrings of that text, so a
/// forward scan locates each row's byte range and the original spans are
/// sliced at it. `hang_indent` indents continuation rows as plain spaces.
pub(crate) fn wrap_line(line: &ContentLine, width: usize) -> Vec<VisualRow> {
    let width = width.max(1);
    let hang = line.hang_indent.min(width.saturating_sub(1));
    let text_width = width - hang;
    let spans = &line.spans;

    if spans.is_empty() {
        return vec![Vec::new()];
    }

    if spans.len() == 1 {
        let hint = spans[0].hint.clone();
        return wrap_text(&spans[0].text, text_width)
            .into_iter()
            .enumerate()
            .map(|(row_index, row)| {
                let mut row_spans = Vec::new();
                if row_index > 0 && hang > 0 {
                    row_spans.push(Span::normal(" ".repeat(hang)));
                }
                row_spans.push(Span::new(row, hint.clone()));
                row_spans
            })
            .collect();
    }

    let full_text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let mut cursor = 0usize;
    let rows: Vec<VisualRow> = wrap_text(&full_text, text_width)
        .into_iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut row_spans = Vec::new();
            if row_index > 0 && hang > 0 {
                row_spans.push(Span::normal(" ".repeat(hang)));
            }
            let Some(start) = full_text[cursor..]
                .find(row.as_str())
                .map(|rel| cursor + rel)
            else {
                row_spans.push(Span::normal(row));
                return row_spans;
            };
            let end = start + row.len();
            cursor = end;
            row_spans.extend(sliced_spans(spans, start, end));
            row_spans
        })
        .collect();

    if rows.is_empty() {
        return vec![Vec::new()];
    }
    rows
}

// endregion: --- Wrap

// region:    --- Paint

/// Paint one visual row at `y`, starting at the buffer's left edge. The row's
/// background covers `width` columns; the gutter and the spans paint over it.
/// Cells outside the buffer are skipped.
pub(crate) fn paint_row(buf: &mut Buffer, y: u16, width: usize, row: &RowPaint<'_>, theme: &Theme) {
    if y >= buf.area.bottom() || width == 0 {
        return;
    }
    let right = buf.area.right();
    let x_end = u16::try_from(width).unwrap_or(u16::MAX).min(right);

    if let Some(bg) = row.bg {
        let bg = theme_color(bg);
        for x in buf.area.left()..x_end {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(bg);
            }
        }
    }

    let mut x = buf.area.left();
    if let Some((gutter, style)) = row.gutter {
        x = put_text(buf, x, y, x_end, gutter, style);
    }
    if let Some((leading, style)) = row.leading {
        x = put_text(buf, x, y, x_end, leading, style);
    }
    for span in row.spans {
        if x >= x_end {
            break;
        }
        let mut style = hint_style(&span.hint, row.base, theme);
        if let Some(bg) = row.bg {
            style = style.bg(theme_color(bg));
        }
        x = put_text(buf, x, y, x_end, &span.text, style);
    }
}

// endregion: --- Paint

// region:    --- Style

/// The single `StyleHint` mapping for this backend. Every markdown, diff, and
/// status hint resolves onto its own semantic surface: the bright success and
/// error colors carry the diff glyphs, the diff *backgrounds* are row surfaces
/// only, and the markdown palette rides [`agnt_render::Markdown`]. `base` is the
/// lane's own text style for the hints that inherit it.
pub(crate) fn hint_style(hint: &StyleHint, base: Style, theme: &Theme) -> Style {
    let md = &theme.markdown;
    match hint {
        StyleHint::Normal | StyleHint::Emphasis => base,
        StyleHint::Meta | StyleHint::Muted => fg(theme.muted),
        StyleHint::Success | StyleHint::DiffAdd => fg(theme.success),
        StyleHint::Error | StyleHint::DiffRemove => fg(theme.error),
        StyleHint::DiffHunk => fg(theme.notice).add_modifier(Modifier::BOLD),
        StyleHint::Cancelled => base,
        StyleHint::MdBold => fg(md.emphasis).add_modifier(Modifier::BOLD),
        StyleHint::MdItalic => fg(md.emphasis).add_modifier(Modifier::ITALIC),
        StyleHint::MdBoldItalic => fg(md.emphasis)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC),
        StyleHint::MdHeading(level) => match level {
            1..=6 => fg(md.heading[usize::from(*level)]).add_modifier(Modifier::BOLD),
            _ => fg(md.heading[0]).add_modifier(Modifier::BOLD),
        },
        StyleHint::MdInlineCode => fg(md.inline_code),
        StyleHint::MdCodeKeyword => fg(md.code_keyword),
        StyleHint::MdCodeType => fg(md.code_type),
        StyleHint::MdCodeFunction => fg(md.code_function),
        StyleHint::MdCodeVariable => fg(md.code_variable),
        StyleHint::MdCodeConstant => fg(md.code_constant),
        StyleHint::MdCodeString => fg(md.code_string),
        StyleHint::MdCodeNumber => fg(md.code_number),
        StyleHint::MdCodeOperator => fg(md.code_operator),
        StyleHint::MdCodePunctuation => fg(md.code_punctuation),
        StyleHint::MdCodeComment => fg(md.code_comment).add_modifier(Modifier::ITALIC),
        StyleHint::MdCodePlain => fg(theme.text),
        StyleHint::MdLink => fg(md.link),
        StyleHint::MdLinkUrl => fg(md.link_url)
            .add_modifier(Modifier::ITALIC)
            .add_modifier(Modifier::UNDERLINED),
        StyleHint::MdQuote => fg(md.quote),
        StyleHint::MdListMarker => fg(md.list_marker),
    }
}

/// A foreground style from a backend-neutral color.
pub(crate) fn fg(color: Color) -> Style {
    Style::default().fg(theme_color(color))
}

/// A backend-neutral color as a terminal color.
pub(crate) fn theme_color(color: Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(color.r, color.g, color.b)
}

/// The row background for a line-level diff tint. A context line keeps the
/// surrounding surface; the add and remove tints are surfaces of their own.
pub(crate) fn diff_tint_bg(tint: Option<&DiffTint>, theme: &Theme) -> Option<Color> {
    match tint {
        Some(DiffTint::Add) => Some(theme.diff_add_bg),
        Some(DiffTint::Remove) => Some(theme.diff_remove_bg),
        Some(DiffTint::Context) | None => None,
    }
}

// endregion: --- Style

// region:    --- Support

/// Write `text` from `x` up to `x_end`, returning the column after the last
/// glyph. Wide graphemes reserve their trailing cell.
fn put_text(buf: &mut Buffer, x: u16, y: u16, x_end: u16, text: &str, style: Style) -> u16 {
    if x >= x_end || text.is_empty() {
        return x;
    }
    let (next_x, _) = buf.set_stringn(x, y, text, usize::from(x_end - x), style);
    next_x
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    textwrap::wrap(
        text,
        textwrap::Options::new(width).word_splitter(textwrap::WordSplitter::NoHyphenation),
    )
    .into_iter()
    .map(|row| row.into_owned())
    .collect()
}

/// Styled spans covering byte range `[start, end)` of the joined span text.
fn sliced_spans(spans: &[Span], start: usize, end: usize) -> Vec<Span> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    for span in spans {
        let span_start = offset;
        let span_end = offset + span.text.len();
        offset = span_end;
        let from = start.max(span_start);
        let to = end.min(span_end);
        if from >= to {
            continue;
        }
        result.push(Span::new(
            span.text[from - span_start..to - span_start].to_string(),
            span.hint.clone(),
        ));
    }
    result
}

// endregion: --- Support

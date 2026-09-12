use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span as RatatuiSpan},
};

use std::collections::HashMap;

use crate::rendering::theme::TuiTheme;
use nu_agent_core::transcript::{
    ir::{ContentLine, RenderBlock, Role, StyleHint},
    renderer::{BlockRenderer, ItemStatus, RenderContext},
};

const IN_PROGRESS_SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct TuiRenderer {
    pub theme: TuiTheme,
}

impl BlockRenderer for TuiRenderer {
    type Output = Vec<Line<'static>>;

    fn render(&self, block: &RenderBlock, ctx: &RenderContext) -> Self::Output {
        // Project markdown at render time using the available canvas width.
        // This ensures tables and other width-sensitive constructs can use the
        // actual terminal width rather than a fixed size baked in at construction.
        let projected_lines: Vec<ContentLine>;
        let content_lines: &[ContentLine] = if let Some(md) = &block.markdown {
            let canvas_width = u16::try_from(ctx.width).unwrap_or(u16::MAX);
            projected_lines = crate::markdown::render_markdown_lines(md, Some(canvas_width));
            &projected_lines
        } else {
            &block.lines
        };

        // Handle empty content as single empty line
        let empty_fallback = [ContentLine::empty()];
        let content_lines = if content_lines.is_empty() {
            empty_fallback.as_slice()
        } else {
            content_lines
        };

        let mut result = Vec::new();

        // Pre-wrap prose so no emitted Line exceeds the pane width; ratatui's
        // Paragraph wrap then never re-wraps, and every visual row keeps its
        // lane prefix (alignment column 4 on continuation rows too).
        let wrap_width = ctx.width.saturating_sub(lane_prefix_width()).max(1);

        for (index, content_line) in content_lines.iter().enumerate() {
            let is_first = index == 0;
            let wrapped_rows = self.wrapped_row_spans(content_line, &block.role, wrap_width);

            for (row_idx, row_spans) in wrapped_rows.into_iter().enumerate() {
                // Build prefix for first row of the first ContentLine; every
                // other row is a continuation: no cursor, no status indicator,
                // and (except for the user rail) a blank role label so the
                // icon never repeats on wrapped rows.
                let is_row_zero = is_first && row_idx == 0;
                let mut spans = if is_row_zero {
                    let mut spans =
                        self.lane_prefix(block.role.clone(), ctx.cursor, block.suppress_prefix);

                    // Add status indicator if present
                    if let Some(status) = &ctx.status {
                        let indicator = Self::indicator_char(status, ctx.now_millis);
                        let style = self.indicator_style(status);
                        spans.push(RatatuiSpan::styled(format!("{indicator} "), style));
                    }

                    spans
                } else {
                    self.lane_prefix_continuation(block.role.clone(), block.suppress_prefix)
                };

                // Add the wrapped content spans for this row
                spans.extend(row_spans);

                // Center the line if requested
                if block.center {
                    let line_char_width: usize =
                        spans.iter().map(|s| s.content.chars().count()).sum();
                    let padding = ctx.width.saturating_sub(line_char_width) / 2;
                    if padding > 0 {
                        let mut padded =
                            vec![RatatuiSpan::styled(" ".repeat(padding), Style::default())];
                        padded.append(&mut spans);
                        spans = padded;
                    }
                }

                // Apply row overlays (selection highlighting, etc.)
                let row_style = self.row_style(&block.role);
                let spans = self.apply_row_overlays(spans, row_style, ctx.selected);

                result.push(Line::from(spans));
            }
        }

        result
    }
}

pub fn lane_prefix_width() -> usize {
    // cursor_str (2 chars: "> " or "  ") + label (2 chars: role icon + space)
    4
}

/// Pre-wrap a single prose row at `width` display columns so no rendered
/// [`Line`] ever exceeds the pane width and ratatui never re-wraps (which
/// would discard the per-row lane prefix). Greedy first-fit on ASCII spaces,
/// matching ratatui's default word wrapper. Returns at least one row, so an
/// empty input still renders as one empty row.
pub(crate) fn wrap_prose(text: &str, width: usize) -> Vec<std::borrow::Cow<'_, str>> {
    textwrap::wrap(
        text,
        textwrap::Options::new(width.max(1)).word_splitter(textwrap::WordSplitter::NoHyphenation),
    )
}

// region:    --- Support

impl TuiRenderer {
    /// Wrap a ContentLine at `wrap_width` display columns and reconstruct the
    /// styled spans of every wrapped row.
    ///
    /// Single-span lines (all prose) wrap one-to-one. Multi-span lines wrap on
    /// the joined span text; textwrap rows are contiguous byte substrings of
    /// that text (it drops only inter-word whitespace at wrap points and inserts
    /// nothing), so a forward scan from the previous row locates each row's byte
    /// range and the original spans are sliced at that range.
    fn wrapped_row_spans(
        &self,
        content_line: &ContentLine,
        role: &Role,
        wrap_width: usize,
    ) -> Vec<Vec<RatatuiSpan<'static>>> {
        let spans = &content_line.spans;
        let hang_indent = content_line.hang_indent.min(wrap_width.saturating_sub(1));
        let text_width = wrap_width - hang_indent;

        if spans.len() == 1 {
            let style = self.hint_to_style(&spans[0].hint, role);
            return wrap_prose(&spans[0].text, text_width)
                .iter()
                .enumerate()
                .map(|(row_idx, row)| {
                    let text = row.to_string();
                    if row_idx > 0 && hang_indent > 0 {
                        // Hang indent is content indentation, not the styled
                        // lane prefix, so a plain unstyled span is correct.
                        let mut row_spans = vec![RatatuiSpan::raw(" ".repeat(hang_indent))];
                        row_spans.push(RatatuiSpan::styled(text, style));
                        row_spans
                    } else {
                        vec![RatatuiSpan::styled(text, style)]
                    }
                })
                .collect();
        }

        let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        let mut cursor = 0usize;
        wrap_prose(&full_text, text_width)
            .iter()
            .enumerate()
            .map(|(row_idx, row)| {
                if row.is_empty() {
                    return Vec::new();
                }
                let mut row_spans = if row_idx > 0 && hang_indent > 0 {
                    vec![RatatuiSpan::raw(" ".repeat(hang_indent))]
                } else {
                    Vec::new()
                };
                let Some(start) = full_text[cursor..]
                    .find(row.as_ref())
                    .map(|rel| cursor + rel)
                else {
                    // Defensive: wrap rows are always substrings of the joined
                    // text. If matching ever fails, render the row unstyled
                    // rather than dropping content.
                    row_spans.push(RatatuiSpan::raw(row.to_string()));
                    return row_spans;
                };
                let end = start + row.len();
                cursor = end;
                row_spans.extend(self.sliced_row_spans(spans, role, start, end));
                row_spans
            })
            .collect()
    }

    /// Styled spans covering byte range `[start, end)` of the joined span text.
    fn sliced_row_spans(
        &self,
        spans: &[nu_agent_core::transcript::ir::Span],
        role: &Role,
        start: usize,
        end: usize,
    ) -> Vec<RatatuiSpan<'static>> {
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
            let text = &span.text[from - span_start..to - span_start];
            result.push(RatatuiSpan::styled(
                text.to_string(),
                self.hint_to_style(&span.hint, role),
            ));
        }
        result
    }
}

// endregion: --- Support

impl TuiRenderer {
    fn lane_prefix(
        &self,
        role: Role,
        cursor: bool,
        suppress_prefix: bool,
    ) -> Vec<RatatuiSpan<'static>> {
        let cursor_str = if cursor { "> " } else { "  " };
        let (label, style) = if suppress_prefix {
            ("  ", self.theme.role_system)
        } else {
            match role {
                Role::User => ("▏ ", self.theme.lane_prefix_user),
                Role::Assistant => ("  ", self.theme.lane_prefix_assistant),
                Role::Tool => ("⚙ ", self.theme.lane_prefix_tool),
                Role::ToolDisplay => ("  ", self.theme.lane_prefix_assistant),
                Role::Compaction => ("~ ", self.theme.lane_prefix_compaction),
                Role::System => ("· ", self.theme.lane_prefix_system),
                Role::Separator => ("  ", self.theme.role_separator),
            }
        };
        vec![
            RatatuiSpan::styled(cursor_str.to_string(), Style::default()),
            RatatuiSpan::styled(label.to_string(), style),
        ]
    }

    /// Lane prefix for wrapped continuation rows: identical lane styling but
    /// a blank 2-char role label, except the user rail which stays on every
    /// row (task 7bd175d2).
    fn lane_prefix_continuation(
        &self,
        role: Role,
        suppress_prefix: bool,
    ) -> Vec<RatatuiSpan<'static>> {
        let (label, style) = if suppress_prefix {
            ("  ", self.theme.role_system)
        } else {
            match role {
                Role::User => ("▏ ", self.theme.lane_prefix_user),
                Role::Assistant => ("  ", self.theme.lane_prefix_assistant),
                Role::Tool => ("  ", self.theme.lane_prefix_tool),
                Role::ToolDisplay => ("  ", self.theme.lane_prefix_assistant),
                Role::Compaction => ("  ", self.theme.lane_prefix_compaction),
                Role::System => ("  ", self.theme.lane_prefix_system),
                Role::Separator => ("  ", self.theme.role_separator),
            }
        };
        vec![
            RatatuiSpan::styled("  ".to_string(), Style::default()),
            RatatuiSpan::styled(label.to_string(), style),
        ]
    }

    fn role_style(&self, role: &Role) -> Style {
        match role {
            Role::User => self.theme.role_user,
            Role::Assistant => self.theme.role_assistant,
            Role::Tool => self.theme.role_tool,
            Role::ToolDisplay => self.theme.role_assistant,
            Role::Compaction => self.theme.role_compaction,
            Role::System => self.theme.role_system,
            Role::Separator => self.theme.role_separator,
        }
    }

    fn row_style(&self, role: &Role) -> Style {
        match role {
            Role::User => self.theme.row_user.bg(self.theme.row_user_bg),
            Role::Assistant => self.theme.row_assistant,
            Role::Tool => self.theme.row_tool,
            Role::ToolDisplay => self.theme.row_assistant,
            Role::Compaction => self.theme.row_compaction,
            Role::System => self.theme.row_system,
            Role::Separator => Style::default(),
        }
    }

    fn hint_to_style(&self, hint: &StyleHint, role: &Role) -> Style {
        match hint {
            StyleHint::Normal | StyleHint::Emphasis => self.role_style(role),
            StyleHint::Meta | StyleHint::Muted => self.theme.tool_meta,
            StyleHint::Success => self.theme.status_done,
            StyleHint::Error => self.theme.status_failed,
            StyleHint::DiffAdd => self.theme.status_done,
            StyleHint::DiffRemove => self.theme.status_failed,
            StyleHint::DiffHunk => self.theme.role_system.add_modifier(Modifier::BOLD),
            StyleHint::Cancelled => self
                .role_style(role)
                .add_modifier(self.theme.cancelled_modifier),
            StyleHint::MdBold => Style::default().add_modifier(Modifier::BOLD),
            StyleHint::MdItalic => Style::default().add_modifier(Modifier::ITALIC),
            StyleHint::MdBoldItalic => Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
            StyleHint::MdInlineCode => self.theme.inline_code,
            StyleHint::MdCodeKeyword => self.theme.syntax_keyword,
            StyleHint::MdCodeType => self.theme.syntax_type,
            StyleHint::MdCodeFunction => self.theme.syntax_function,
            StyleHint::MdCodeVariable => self.theme.syntax_variable,
            StyleHint::MdCodeConstant => self.theme.syntax_constant,
            StyleHint::MdCodeString => self.theme.syntax_string,
            StyleHint::MdCodeNumber => self.theme.syntax_number,
            StyleHint::MdCodeOperator => self.theme.syntax_operator,
            StyleHint::MdCodePunctuation => self.theme.syntax_punctuation,
            StyleHint::MdCodeComment => self.theme.syntax_comment,
            StyleHint::MdCodePlain => Style::default(),
        }
    }

    fn indicator_char(status: &ItemStatus, now_millis: u128) -> &'static str {
        match status {
            ItemStatus::InProgress => {
                let idx = ((now_millis / 100) % IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
                IN_PROGRESS_SPINNER_FRAMES[idx]
            }
            ItemStatus::Done => "✓",
            ItemStatus::Failed => "✕",
            ItemStatus::Queued => "•",
            ItemStatus::Cancelled => "✕",
            ItemStatus::Unknown => "?",
        }
    }

    fn indicator_style(&self, status: &ItemStatus) -> Style {
        match status {
            ItemStatus::InProgress => self.theme.status_running,
            ItemStatus::Done => self.theme.status_done,
            ItemStatus::Failed => self.theme.status_failed,
            ItemStatus::Queued => self.theme.status_queued,
            ItemStatus::Cancelled => self.theme.status_cancelled,
            ItemStatus::Unknown => self.theme.status_queued,
        }
    }

    fn apply_row_overlays(
        &self,
        spans: Vec<RatatuiSpan<'static>>,
        row_style: Style,
        selected: bool,
    ) -> Vec<RatatuiSpan<'static>> {
        spans
            .into_iter()
            .map(|span| {
                let mut style = span.style.patch(row_style);
                if selected {
                    style = style.patch(self.theme.selection_bg);
                }
                RatatuiSpan::styled(span.content.into_owned(), style)
            })
            .collect()
    }

    pub fn render_cached(
        &self,
        block: &RenderBlock,
        ctx: &RenderContext,
        cache: &mut HashMap<String, Vec<ContentLine>>,
    ) -> Vec<Line<'static>> {
        let block = if let Some(md) = &block.markdown {
            let content_lines = cache.entry(md.clone()).or_insert_with(|| {
                let canvas_width = u16::try_from(ctx.width).unwrap_or(u16::MAX);
                crate::markdown::render_markdown_lines(md, Some(canvas_width))
            });
            RenderBlock {
                role: block.role.clone(),
                lines: content_lines.clone(),
                markdown: None,
                center: block.center,
                suppress_prefix: block.suppress_prefix,
            }
        } else {
            block.clone()
        };
        self.render(&block, ctx)
    }
}

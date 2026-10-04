use ratatui::{
    style::Style,
    text::{Line, Span as RatatuiSpan},
};

use crate::rendering::theme::{TuiTheme, diff_tint_to_bg, hint_to_style};
use nu_agent_core::transcript::ir::{Block, ContentLine, Lane};
use nu_agent_core::transcript::renderer::{FrameContext, ItemStatus};

pub struct TuiRenderer {
    pub theme: TuiTheme,
}

pub fn lane_prefix_width() -> usize {
    // cursor_str (2 chars: "> " or "  ") + label (2 chars: lane icon + space)
    4
}

/// Pre-wrap a single text row at `width` display columns so no rendered
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

/// The pure-function renderer (task 6a7a3916): projects the Block's source at
/// `ctx.width`, wraps at the shared wrap budget, applies the lane prefix on
/// row 0 (blank label on continuations, user rail on every row), the status
/// indicator, and the fill (Full → user bg on content rows, Code → margin
/// rows + surface0 bg, None → nothing). Pure: same inputs, same outputs.
pub fn layout(block: &Block, ctx: &FrameContext, theme: &TuiTheme) -> Vec<Line<'static>> {
    let renderer = TuiRenderer {
        theme: theme.clone(),
    };
    renderer.render_block(block, ctx)
}

/// Row-count twin of [`layout`] (task 53279c82): the exact number of visual
/// rows [`layout`] would produce for the same Block at the same width —
/// projection, per-line wrapping at the shared wrap budget (status indicator
/// included), and the 2 margin rows for Fill::Code. Used by the render loop
/// for scroll math; must stay in lockstep with [`TuiRenderer::render_block`]
/// by construction.
pub fn measure(block: &Block, width: usize, theme: &TuiTheme) -> usize {
    let renderer = TuiRenderer {
        theme: theme.clone(),
    };
    renderer.measure_block(block, width)
}

impl TuiRenderer {
    /// Row-count mirror of [`TuiRenderer::render_block`]: projects the same
    /// source, wraps each ContentLine at the same wrap budget (status-indicator
    /// shrink included — the indicator sits on row 0 only, so only row 0's
    /// ContentLine wraps with the reduced budget), and adds the Fill::Code
    /// margin rows. Same inputs, same count as `render_block(..).len()`.
    fn measure_block(&self, block: &Block, width: usize) -> usize {
        let mut content_lines = block.source.project(width);
        if content_lines.is_empty() {
            content_lines.push(ContentLine::empty());
        }

        let has_status = block.status.is_some();
        let wrap_width = crate::state::code_block::content_wrap_width(width, has_status);

        let lane = LaneContext::from(block, &self.theme);
        let mut content_rows = 0usize;
        for content_line in &content_lines {
            let is_row_zero = content_rows == 0;
            let wrapped = self.wrapped_row_count(content_line, &lane, wrap_width, is_row_zero);
            content_rows += wrapped;
        }

        // Fill::Code injects one filled margin row above and below the block.
        let margin = block.fill.margin_row_count();

        content_rows + margin
    }

    /// Visual row count a single ContentLine wraps into. Row 0 uses the wrap
    /// budget that already accounts for the status indicator (mirroring
    /// `render_block`, which renders row 0 without a lane-marker slot);
    /// continuations use the plain budget.
    fn wrapped_row_count(
        &self,
        content_line: &ContentLine,
        lane: &LaneContext,
        wrap_width: usize,
        is_row_zero: bool,
    ) -> usize {
        let _ = lane;
        let _ = is_row_zero;
        let spans = &content_line.spans;
        let hang_indent = content_line.hang_indent.min(wrap_width.saturating_sub(1));
        let text_width = wrap_width - hang_indent;

        let rows = if spans.len() == 1 {
            wrap_prose(&spans[0].text, text_width).len()
        } else {
            let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
            wrap_prose(&full_text, text_width).len()
        };
        rows.max(1)
    }

    fn render_block(&self, block: &Block, ctx: &FrameContext) -> Vec<Line<'static>> {
        let mut content_lines = block.source.project(ctx.width);
        if content_lines.is_empty() {
            content_lines.push(ContentLine::empty());
        }

        let has_status = block.status.is_some();
        let wrap_width = crate::state::code_block::content_wrap_width(ctx.width, has_status);

        let lane = LaneContext::from(block, &self.theme);
        let mut result = Vec::new();

        for content_line in &content_lines {
            let is_first_content_row = result.is_empty();
            let wrapped_rows = self.wrapped_row_spans(content_line, &lane, wrap_width);

            for (row_idx, row_spans) in wrapped_rows.into_iter().enumerate() {
                let is_row_zero = is_first_content_row && row_idx == 0;
                let mut spans = if is_row_zero {
                    let mut spans = self.lane_prefix(&lane, ctx.cursor);

                    if let Some(status) = &block.status {
                        let indicator = status.indicator_char(ctx.now_millis);
                        let style = self.indicator_style(status);
                        spans.push(RatatuiSpan::styled(format!("{indicator} "), style));
                    }

                    spans
                } else {
                    self.lane_prefix_continuation(&lane)
                };

                spans.extend(row_spans);

                // Center banner lines. Centering follows the lane: only
                // `Lane::SystemBlank` (which `Banner::lane()` returns) is
                // centered, so the renderer never inspects `block.source`
                // for layout. `Banner` is the only type that returns this
                // lane.
                if block.lane == Lane::SystemBlank {
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

                let row_style = self.row_style_for(block, &lane, is_row_zero);
                // A line-level diff tint is a row background: it wins over the
                // block's fill and covers the lane prefix too, so the whole row
                // is tinted and the render loop's full-width paint keeps it.
                let row_style = match content_line
                    .diff_tint
                    .as_ref()
                    .and_then(|tint| diff_tint_to_bg(tint, &self.theme))
                {
                    Some(bg) => row_style.bg(bg),
                    None => row_style,
                };
                let spans = self.apply_row_overlays(spans, row_style, ctx.selected);

                result.push(Line::from(spans));
            }
        }

        if result.is_empty() {
            result.push(Line::from(""));
        }

        // Fill::Code inserts a margin row above and below the block so the
        // code surface has internal padding. Each margin row carries an empty
        // span styled with surface0 — a bare `Line::from("")` has no span, so
        // the full-width paint in the render loop would skip it (task
        // 69bd5698). `Fill` owns the count (2 for Code, 0 otherwise); the
        // renderer owns the colour and placement. Every other fill inserts
        // no margins.
        if block.fill.margin_row_count() > 0 {
            result = Self::with_margin_rows(result, self.theme.surface0);
        }

        result
    }

    /// Insert one filled margin row above and below `lines` so a code/diff
    /// block has internal padding around its text. Each margin row carries a
    /// single empty span styled with `bg` — a bare `Line::from("")` has no
    /// span, so the render loop's full-width paint would skip it and the
    /// margin would render unfilled (task 69bd5698).
    fn with_margin_rows(
        lines: Vec<Line<'static>>,
        bg: ratatui::style::Color,
    ) -> Vec<Line<'static>> {
        let margin = || {
            Line::from(vec![RatatuiSpan::styled(
                String::new(),
                Style::default().bg(bg),
            )])
        };
        let mut out = Vec::with_capacity(lines.len() + 2);
        out.push(margin());
        out.extend(lines);
        out.push(margin());
        out
    }

    /// Wrap a ContentLine at `wrap_width` display columns and reconstruct the
    /// styled spans of every wrapped row.
    ///
    /// Single-span lines (all prose) wrap one-to-one. Multi-span lines wrap on
    /// the joined span text; textwrap rows are contiguous byte substrings of
    /// that text (it drops only inter-word whitespace at wrap points and inserts
    /// nothing), so a forward scan from the previous row locates each row's byte
    /// range and the original spans are sliced at that range.
    ///
    /// A line-level `DiffTint` is not applied here: it is a row background,
    /// applied by `render_block` via the row style so it covers the lane prefix
    /// and survives the render loop's full-width paint.
    fn wrapped_row_spans(
        &self,
        content_line: &ContentLine,
        lane: &LaneContext,
        wrap_width: usize,
    ) -> Vec<Vec<RatatuiSpan<'static>>> {
        let spans = &content_line.spans;
        let hang_indent = content_line.hang_indent.min(wrap_width.saturating_sub(1));
        let text_width = wrap_width - hang_indent;

        if spans.len() == 1 {
            let style = hint_to_style(&spans[0].hint, lane.role_style, &self.theme);
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
                row_spans.extend(self.sliced_row_spans(spans, lane, start, end));
                row_spans
            })
            .collect()
    }

    /// Styled spans covering byte range `[start, end)` of the joined span text.
    fn sliced_row_spans(
        &self,
        spans: &[nu_agent_core::transcript::ir::Span],
        lane: &LaneContext,
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
                hint_to_style(&span.hint, lane.role_style, &self.theme),
            ));
        }
        result
    }

    fn lane_prefix(&self, lane: &LaneContext, cursor: bool) -> Vec<RatatuiSpan<'static>> {
        let cursor_str = if cursor { "> " } else { "  " };
        vec![
            RatatuiSpan::styled(cursor_str.to_string(), Style::default()),
            RatatuiSpan::styled(lane.marker.clone(), lane.marker_style),
        ]
    }

    /// Lane prefix for wrapped continuation rows: identical lane styling but
    /// a blank marker — except the user rail, which stays on every row so a
    /// multi-row user block keeps a constant gutter (task 667e4926, task
    /// 7bd175d2).
    fn lane_prefix_continuation(&self, lane: &LaneContext) -> Vec<RatatuiSpan<'static>> {
        vec![
            RatatuiSpan::styled("  ".to_string(), Style::default()),
            RatatuiSpan::styled(lane.continuation_marker.to_string(), lane.marker_style),
        ]
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

    /// Row style for a rendered row. `Fill::Code` paints the surface0
    /// background on content rows only — row 0 (the call line plus any status
    /// indicator) keeps the lane's own row style so the header stays untinted.
    /// Fill::Full and Fill::None always defer to [`LaneContext::row_style`],
    /// which carries the user background for the user lane and the per-role
    /// row style (row_assistant/row_tool/row_compaction/row_system) otherwise.
    ///
    /// The fill *colour* (surface0) lives here, not on `Fill`: `Fill` is in
    /// nu-agent-core, which has no theme. `Fill` owns "do I have a code
    /// background"; the renderer owns "code background is surface0, and only
    /// content rows get it".
    fn row_style_for(&self, block: &Block, lane: &LaneContext, is_row_zero: bool) -> Style {
        use nu_agent_core::transcript::ir::Fill;
        match block.fill {
            Fill::Code if !is_row_zero => Style::default().bg(self.theme.surface0),
            Fill::Code | Fill::Full | Fill::None => lane.row_style,
        }
    }

    fn apply_row_overlays(
        &self,
        spans: Vec<RatatuiSpan<'static>>,
        row_style: Style,
        selected: bool,
    ) -> Vec<RatatuiSpan<'static>> {
        let row_bg = row_style.bg;
        spans
            .into_iter()
            .map(|span| {
                let mut style = span.style;
                if let Some(bg) = row_bg {
                    style = style.bg(bg);
                }
                if selected {
                    style = style.patch(self.theme.selection_bg);
                }
                RatatuiSpan::styled(span.content.into_owned(), style)
            })
            .collect()
    }
}

// region:    --- Support

/// Lane styling context derived from a Block at construction time. Replaces
/// the old `Role`-based match in the renderer: the lane was already decided,
/// so the renderer only reads it. Carries the two style dimensions the old
/// renderer had: `role_style` (text foreground via `hint_to_style`) and
/// `row_style` (row background via `apply_row_overlays`).
struct LaneContext {
    /// The 2-char marker slot for row 0 (icon + space, or blanks).
    marker: String,
    /// The marker slot for continuation rows (blank unless user rail).
    continuation_marker: &'static str,
    marker_style: Style,
    role_style: Style,
    row_style: Style,
}

impl LaneContext {
    /// Lane styling from the lane variant alone — the renderer never inspects
    /// `block.source`. A block's lane is frozen at construction time and
    /// already encodes the styling the source needs (task 46ca79fe).
    fn from(block: &Block, theme: &TuiTheme) -> Self {
        match block.lane {
            Lane::Marker(icon) => {
                let (marker_style, role_style, row_style) = match icon {
                    "▏" => (
                        theme.lane_prefix_user,
                        theme.role_user,
                        theme.row_user.bg(theme.row_user_bg),
                    ),
                    "⚙" => (theme.lane_prefix_tool, theme.role_tool, theme.row_tool),
                    "~" => (
                        theme.lane_prefix_compaction,
                        theme.role_compaction,
                        theme.row_compaction,
                    ),
                    "·" => (
                        theme.lane_prefix_system,
                        theme.role_system,
                        theme.row_system,
                    ),
                    _ => (
                        theme.lane_prefix_system,
                        theme.role_system,
                        theme.row_system,
                    ),
                };
                // The user rail stays on every row (the pre-refactor
                // `Role::User => "▏ "` continuation label); other marker
                // lanes render a blank continuation slot. This keeps a
                // constant gutter down a multi-row user block (task 667e4926).
                let continuation_marker = if icon == "▏" { "▏ " } else { "  " };
                Self {
                    marker: format!("{icon} "),
                    continuation_marker,
                    marker_style,
                    role_style,
                    row_style,
                }
            }
            // Assistant prose and spacers: blank prefix, assistant styling.
            Lane::Blank => Self {
                marker: "  ".to_string(),
                continuation_marker: "  ",
                marker_style: theme.lane_prefix_assistant,
                role_style: theme.role_assistant,
                row_style: theme.row_assistant,
            },
            // Banner (startup logo): blank prefix, system styling — the old
            // Logo `Role::System` (`role_system` text, `row_system` row, and
            // the suppressed prefix span in `role_system`).
            Lane::SystemBlank => Self {
                marker: "  ".to_string(),
                continuation_marker: "  ",
                marker_style: theme.role_system,
                role_style: theme.role_system,
                row_style: theme.row_system,
            },
        }
    }
}

// endregion: --- Support

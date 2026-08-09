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
        let content_lines: &[ContentLine] = if let Some(ref md) = block.markdown {
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

        for (index, content_line) in content_lines.iter().enumerate() {
            let is_first = index == 0;

            // Build prefix for first line
            let mut spans = if is_first {
                let mut spans =
                    self.lane_prefix(block.role.clone(), ctx.cursor, block.suppress_prefix);

                // Add status indicator if present
                if let Some(ref status) = ctx.status {
                    let indicator = Self::indicator_char(status, ctx.now_millis);
                    let style = self.indicator_style(status);
                    spans.push(RatatuiSpan::styled(format!("{} ", indicator), style));
                }

                spans
            } else {
                // Subsequent lines: no cursor, no status indicator
                self.lane_prefix(block.role.clone(), false, block.suppress_prefix)
            };

            // Add content spans
            for span in &content_line.spans {
                let style = self.hint_to_style(&span.hint, &block.role);
                spans.push(RatatuiSpan::styled(span.text.clone(), style));
            }

            // Center the line if requested
            if block.center {
                let line_char_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
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

        result
    }
}

pub fn lane_prefix_width() -> usize {
    // cursor_str (2 chars: "> " or "  ") + label (2 chars: role icon + space)
    4
}

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
        }
    }

    fn indicator_style(&self, status: &ItemStatus) -> Style {
        match status {
            ItemStatus::InProgress => self.theme.status_running,
            ItemStatus::Done => self.theme.status_done,
            ItemStatus::Failed => self.theme.status_failed,
            ItemStatus::Queued => self.theme.status_queued,
            ItemStatus::Cancelled => self.theme.status_cancelled,
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
        let block = if let Some(ref md) = block.markdown {
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

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span as RatatuiSpan},
};

use crate::agent::ui::tui::rendering::theme::TuiTheme;

use super::{
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
        // Special case: Separator role with empty lines (horizontal rule)
        if block.role == Role::Separator && block.lines.is_empty() {
            let width = ctx.width.saturating_sub(4).max(1);
            let mut spans = self.lane_prefix(Role::Separator, ctx.cursor);
            spans.push(RatatuiSpan::styled(
                "─".repeat(width),
                self.theme.role_separator,
            ));
            let spans = self.apply_row_overlays(spans, Style::default(), ctx.selected);
            return vec![Line::from(spans)];
        }

        // Handle empty block.lines as single empty line
        let content_lines = if block.lines.is_empty() {
            vec![ContentLine::empty()]
        } else {
            block.lines.clone()
        };

        let mut result = Vec::new();

        for (index, content_line) in content_lines.iter().enumerate() {
            let is_first = index == 0;

            // Build prefix for first line
            let mut spans = if is_first {
                let mut spans = self.lane_prefix(block.role.clone(), ctx.cursor);

                // Add status indicator if present
                if let Some(ref status) = ctx.status {
                    let indicator = Self::indicator_char(status, ctx.now_millis);
                    let style = self.indicator_style(status);
                    spans.push(RatatuiSpan::styled(format!("{} ", indicator), style));
                }

                spans
            } else {
                // Subsequent lines: no cursor, no status indicator
                self.lane_prefix(block.role.clone(), false)
            };

            // Add content spans
            for span in &content_line.spans {
                let style = self.hint_to_style(&span.hint, &block.role);
                spans.push(RatatuiSpan::styled(span.text.clone(), style));
            }

            // Apply row overlays (selection highlighting, etc.)
            let row_style = self.row_style(&block.role);
            let spans = self.apply_row_overlays(spans, row_style, ctx.selected);

            result.push(Line::from(spans));
        }

        result
    }
}

impl TuiRenderer {
    fn lane_prefix(&self, role: Role, cursor: bool) -> Vec<RatatuiSpan<'static>> {
        let cursor_str = if cursor { "> " } else { "  " };
        let (label, style) = match role {
            Role::User => ("▏ ", self.theme.lane_prefix_user),
            Role::Assistant => ("  ", self.theme.lane_prefix_assistant),
            Role::Tool => ("⚙ ", self.theme.lane_prefix_tool),
            Role::ToolDisplay => ("  ", self.theme.lane_prefix_assistant),
            Role::System => ("· ", self.theme.lane_prefix_system),
            Role::Separator => ("  ", self.theme.role_separator),
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
            Role::System => self.theme.role_system,
            Role::Separator => self.theme.role_separator,
        }
    }

    fn row_style(&self, role: &Role) -> Style {
        match role {
            Role::User => self.theme.row_user,
            Role::Assistant => self.theme.row_assistant,
            Role::Tool => self.theme.row_tool,
            Role::ToolDisplay => self.theme.row_assistant,
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
            StyleHint::Rendered(style) => *style,
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
}

use ratatui::text::{Line, Span};

use agnt_render::transcript::ir::ContentLine;

use crate::rendering::theme::TuiTheme;
use crate::runtime::panels::wrap_panel_lines;
use crate::runtime::render::UnhandledPayload;
use crate::state::{AppState, Availability, CommandAvailability, PickerPayload, PickerRenderKind};
use crate::widgets::text::hint_style;

use super::content::build_status_lines;

pub(crate) fn help_panel_lines(theme: &TuiTheme, width: u16) -> (&'static str, Vec<Line<'static>>) {
    let lines =
        crate::markdown::project_markdown_to_lines(help_panel_markdown_source(), Some(width))
            .into_iter()
            .map(|line| content_line_to_ratatui_line(line, theme))
            .collect();
    ("Help", wrap_panel_lines(lines, width))
}

fn content_line_to_ratatui_line(line: ContentLine, theme: &TuiTheme) -> Line<'static> {
    // The help panel is role-free prose: subtle_meta is the text style for
    // Normal/Emphasis hints here. Hints resolve through the same vocabulary the
    // transcript widgets paint with.
    let neutral = agnt_render::Theme::from(theme);
    Line::from(
        line.spans
            .into_iter()
            .map(|span| {
                ratatui::text::Span::styled(
                    span.text,
                    hint_style(&span.hint, theme.subtle_meta, &neutral),
                )
            })
            .collect::<Vec<_>>(),
    )
}

pub(super) fn help_panel_markdown_source() -> &'static str {
    include_str!("../help/help.md")
}

pub(crate) fn status_panel_lines(
    state: &AppState,
    width: u16,
) -> (&'static str, Vec<Line<'static>>) {
    let lines = build_status_lines(state)
        .into_iter()
        .map(Line::from)
        .collect();
    ("Status", wrap_panel_lines(lines, width))
}

pub(crate) fn inline_slash_lines_for_render(
    state: &AppState,
    theme: &TuiTheme,
) -> Vec<Line<'static>> {
    if state.picker.render_kind() != Some(PickerRenderKind::InlineSlash) {
        return Vec::new();
    }
    let Some(picker_state) = state.picker.active_state() else {
        return Vec::new();
    };

    let unhandled = UnhandledPayload::new("inline slash list");
    picker_state
        .options
        .iter()
        .enumerate()
        .map(|(idx, opt)| {
            let marker = if idx == picker_state.selection {
                "❯"
            } else {
                " "
            };
            let marker_span = if idx == picker_state.selection {
                Span::styled(marker, theme.focus)
            } else {
                Span::raw(marker)
            };
            let PickerPayload::Slash { command, .. } = &opt.payload else {
                unhandled.warn(&opt.payload);
                return Line::from(vec![
                    marker_span,
                    Span::raw(" "),
                    Span::raw(opt.display.clone()),
                ]);
            };
            let label = command.label();
            let mut spans = vec![
                marker_span,
                Span::raw(" "),
                Span::styled(label.to_string(), theme.subtle_meta),
                Span::raw(" — "),
                Span::styled(command.summary().to_string(), theme.tool_meta),
            ];
            // A command that cannot run now is drawn in the picker's one dimmed
            // vocabulary — the style the model picker puts on a row it cannot
            // switch to. The row keeps its label and summary: the style alone says
            // it cannot act, and the reason stays the submit lane's refusal text.
            if matches!(
                command.availability(&state.status),
                Availability::Unavailable(_)
            ) {
                for span in &mut spans {
                    span.style = theme.picker_row_unavailable;
                }
            }
            Line::from(spans)
        })
        .collect()
}

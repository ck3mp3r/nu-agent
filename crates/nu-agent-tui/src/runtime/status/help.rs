use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use nu_agent_core::transcript::ir::{ContentLine, StyleHint};

use crate::rendering::theme::TuiTheme;
use crate::state::{AppState, PickerPayload, PickerRenderKind};

use super::content::build_status_lines;

pub(crate) fn help_panel_lines(theme: &TuiTheme) -> (&'static str, Vec<Line<'static>>) {
    (
        "Help",
        crate::markdown::project_markdown_to_lines(help_panel_markdown_source(), None)
            .into_iter()
            .map(|line| content_line_to_ratatui_line(line, theme))
            .collect(),
    )
}

fn content_line_to_ratatui_line(line: ContentLine, theme: &TuiTheme) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| ratatui::text::Span::styled(span.text, hint_to_style(&span.hint, theme)))
            .collect::<Vec<_>>(),
    )
}

fn hint_to_style(hint: &StyleHint, theme: &TuiTheme) -> Style {
    match hint {
        StyleHint::Normal | StyleHint::Emphasis => theme.subtle_meta,
        StyleHint::Meta | StyleHint::Muted => theme.tool_meta,
        StyleHint::Success => theme.status_done,
        StyleHint::Error => theme.status_failed,
        StyleHint::DiffAdd => theme.status_done,
        StyleHint::DiffRemove => theme.status_failed,
        StyleHint::DiffHunk => theme.role_system.add_modifier(Modifier::BOLD),
        StyleHint::Cancelled => theme.role_system.add_modifier(theme.cancelled_modifier),
        StyleHint::MdBold => Style::default().add_modifier(Modifier::BOLD),
        StyleHint::MdItalic => Style::default().add_modifier(Modifier::ITALIC),
        StyleHint::MdBoldItalic => Style::default()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC),
        StyleHint::MdInlineCode => theme.inline_code,
        StyleHint::MdCodeKeyword => theme.syntax_keyword,
        StyleHint::MdCodeType => theme.syntax_type,
        StyleHint::MdCodeFunction => theme.syntax_function,
        StyleHint::MdCodeVariable => theme.syntax_variable,
        StyleHint::MdCodeConstant => theme.syntax_constant,
        StyleHint::MdCodeString => theme.syntax_string,
        StyleHint::MdCodeNumber => theme.syntax_number,
        StyleHint::MdCodeOperator => theme.syntax_operator,
        StyleHint::MdCodePunctuation => theme.syntax_punctuation,
        StyleHint::MdCodeComment => theme.syntax_comment,
        StyleHint::MdCodePlain => Style::default(),
    }
}

pub(super) fn help_panel_markdown_source() -> &'static str {
    include_str!("../help/help.md")
}

pub(crate) fn status_panel_lines(state: &AppState) -> (&'static str, Vec<Line<'static>>) {
    let lines = build_status_lines(state)
        .into_iter()
        .map(Line::from)
        .collect();
    ("Status", lines)
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

    picker_state
        .options
        .iter()
        .enumerate()
        .map(|(idx, opt)| {
            let command = match &opt.payload {
                PickerPayload::Slash(c) => *c,
                _ => unreachable!(),
            };
            let marker = if idx == picker_state.selection {
                "❯"
            } else {
                " "
            };
            let label = command.label();
            let summary = command.summary();
            let marker_span = if idx == picker_state.selection {
                Span::styled(marker, theme.focus)
            } else {
                Span::raw(marker)
            };
            Line::from(vec![
                marker_span,
                Span::raw(" "),
                Span::styled(label.to_string(), theme.subtle_meta),
                Span::raw(" — "),
                Span::styled(summary.to_string(), theme.tool_meta),
            ])
        })
        .collect()
}

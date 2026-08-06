use super::*;
use crate::rendering::theme::TuiTheme;

pub(crate) fn help_panel_lines() -> (&'static str, Vec<Line<'static>>) {
    (
        "Help",
        crate::markdown::project_markdown_to_lines(
            help_panel_markdown_source(),
            None,
            &TuiTheme::default(),
        ),
    )
}

pub(super) fn help_panel_markdown_source() -> &'static str {
    include_str!("help/help.md")
}

pub(crate) fn status_panel_lines(
    state: &AppState,
    active_model_identity: &str,
) -> (&'static str, Vec<Line<'static>>) {
    let lines = build_status_lines(state, active_model_identity)
        .into_iter()
        .map(Line::from)
        .collect();
    ("Status", lines)
}

pub(crate) fn inline_slash_lines_for_render(state: &AppState) -> Vec<Line<'static>> {
    if !state.inline_slash_open {
        return Vec::new();
    }

    state
        .inline_slash_suggestions()
        .iter()
        .enumerate()
        .map(|(idx, command)| {
            let marker = if idx == state.inline_slash_selection {
                "❯"
            } else {
                " "
            };
            let label = command.label();
            let summary = command.summary();
            Line::from(format!("{marker} {label} — {summary}"))
        })
        .collect()
}

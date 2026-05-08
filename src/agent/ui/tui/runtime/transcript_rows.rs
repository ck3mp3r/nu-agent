use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::agent::ui::tui::{
    rendering::theme::TuiTheme,
    state::{PromptStatus, ToolCallStatus, TranscriptLine, TranscriptLineStatus, TranscriptRole},
};

pub(super) fn transcript_role_style(role: TranscriptRole) -> Style {
    let theme = TuiTheme::default();
    match role {
        TranscriptRole::User => theme.role_user,
        TranscriptRole::Assistant => theme.role_assistant,
        TranscriptRole::System => theme.role_system,
        TranscriptRole::Tool => theme.role_tool,
        TranscriptRole::Separator => theme.role_separator,
    }
}

pub(super) fn transcript_row_style(role: TranscriptRole, theme: &TuiTheme) -> Style {
    match role {
        TranscriptRole::User => theme.row_user,
        TranscriptRole::Assistant => theme.row_assistant,
        TranscriptRole::Tool => theme.row_tool,
        TranscriptRole::System => theme.row_system,
        TranscriptRole::Separator => Style::default(),
    }
}

pub(super) fn lane_prefix_spans(
    role: TranscriptRole,
    cursor_line: bool,
    theme: &TuiTheme,
) -> Vec<Span<'static>> {
    let cursor = if cursor_line { "> " } else { "  " };
    let (lane_label, lane_style) = match role {
        TranscriptRole::User => ("▏ ", theme.lane_prefix_user),
        TranscriptRole::Assistant => ("  ", theme.lane_prefix_assistant),
        TranscriptRole::Tool => ("⚒ ", theme.lane_prefix_tool),
        TranscriptRole::System => ("· ", theme.lane_prefix_system),
        TranscriptRole::Separator => ("  ", theme.role_separator),
    };

    vec![
        Span::styled(cursor.to_string(), Style::default()),
        Span::styled(lane_label.to_string(), lane_style),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolRowParts {
    label: String,
    metadata: Option<String>,
}

pub(super) fn parse_tool_row_parts(text: &str) -> ToolRowParts {
    if !(text.starts_with("tool[") && text.contains(']')) {
        return ToolRowParts {
            label: text.to_string(),
            metadata: None,
        };
    }

    let closing = text.find(']').unwrap_or(text.len().saturating_sub(1));
    let label = text[..=closing].to_string();
    let tail = text[closing + 1..].trim();
    let metadata = if tail.is_empty() {
        None
    } else {
        Some(tail.to_string())
    };

    ToolRowParts { label, metadata }
}

fn suppress_redundant_done_metadata(
    metadata: String,
    line_status: Option<TranscriptLineStatus>,
) -> Option<String> {
    let is_done = line_status == Some(TranscriptLineStatus::Tool(ToolCallStatus::Done));
    if !is_done {
        return Some(metadata);
    }

    let trimmed = metadata.trim_end();
    let without_done = trimmed.strip_suffix(" · done").unwrap_or(trimmed).trim_end();
    if without_done.is_empty() {
        None
    } else {
        Some(without_done.to_string())
    }
}

pub(super) fn build_row_spans(
    line: &TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    cursor_line: bool,
    selected: bool,
    now_millis: u128,
    theme: &TuiTheme,
    show_status_indicator: bool,
) -> Vec<Span<'static>> {
    let role_style = transcript_role_style(line.role);
    let prompt_modifier = if line_status == Some(TranscriptLineStatus::Prompt(PromptStatus::Cancelled))
    {
        theme.cancelled_modifier
    } else {
        Modifier::empty()
    };

    let mut spans = lane_prefix_spans(line.role, cursor_line, theme)
        .into_iter()
        .map(|span| {
            let style = span.style.add_modifier(prompt_modifier);
            Span::styled(span.content.into_owned(), style)
        })
        .collect::<Vec<_>>();

    if let Some(status) = line_status.filter(|_| show_status_indicator) {
        spans.push(Span::styled(
            format!("{} ", indicator_for_line_status(status, now_millis)),
            indicator_style_for_status(status, theme).add_modifier(prompt_modifier),
        ));
    }

    if let Some(rendered) = line.rendered.as_ref() {
        spans.extend(rendered.spans.iter().map(|span| {
            Span::styled(
                span.content.as_ref().to_string(),
                role_style.patch(span.style).add_modifier(prompt_modifier),
            )
        }));
    } else if line.role == TranscriptRole::Tool {
        let parts = parse_tool_row_parts(&line.text);
        spans.push(Span::styled(parts.label, role_style.add_modifier(prompt_modifier)));
        if let Some(metadata) = parts
            .metadata
            .and_then(|metadata| suppress_redundant_done_metadata(metadata, line_status))
        {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                metadata,
                theme.tool_meta.add_modifier(prompt_modifier),
            ));
        }
    } else {
        spans.push(Span::styled(
            line.text.clone(),
            role_style.add_modifier(prompt_modifier),
        ));
    }

    let row_style = transcript_row_style(line.role, theme).add_modifier(prompt_modifier);
    apply_row_style_overlays(spans, row_style, selected, theme)
}

pub(super) fn apply_row_style_overlays(
    spans: Vec<Span<'static>>,
    row_style: Style,
    selected: bool,
    theme: &TuiTheme,
) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|span| {
            Span::styled(
                span.content.into_owned(),
                style_with_row_overlays(span.style, row_style, selected, theme),
            )
        })
        .collect()
}

pub(super) fn style_with_row_overlays(
    style: Style,
    row_style: Style,
    selected: bool,
    theme: &TuiTheme,
) -> Style {
    let mut patched = style.patch(row_style);
    if selected {
        patched = patched.patch(theme.selection_bg);
    }
    patched
}

pub(super) fn render_transcript_lines(
    line: TranscriptLine,
    content_width: usize,
    selected: bool,
    cursor_line: bool,
    line_status: Option<TranscriptLineStatus>,
    now_millis: u128,
    theme: &TuiTheme,
) -> Vec<Line<'static>> {
    if line.role == TranscriptRole::Separator {
        let width = content_width.saturating_sub(4).max(1);
        let desired = line
            .text
            .chars()
            .next()
            .map(|ch| ch.to_string().repeat(width))
            .unwrap_or_else(|| "-".repeat(width));
        let mut spans = lane_prefix_spans(TranscriptRole::Separator, cursor_line, theme);
        spans.push(Span::styled(desired, theme.role_separator));
        spans = apply_row_style_overlays(spans, Style::default(), selected, theme);
        return vec![Line::from(spans)];
    }

    if line.rendered.is_none() && line.text.contains('\n') {
        let prompt_modifier =
            if line_status == Some(TranscriptLineStatus::Prompt(PromptStatus::Cancelled)) {
                theme.cancelled_modifier
            } else {
                Modifier::empty()
            };

        let mut rendered = Vec::new();
        for (idx, chunk) in line.text.split('\n').enumerate() {
            let chunk_line = TranscriptLine {
                role: line.role,
                text: chunk.to_string(),
                rendered: None,
            };
            let mut spans = build_row_spans(
                &chunk_line,
                line_status,
                cursor_line && idx == 0,
                selected,
                now_millis,
                theme,
                idx == 0,
            );

            let used_width = spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum::<usize>();
            if used_width < content_width {
                let mut pad_style = transcript_row_style(line.role, theme).add_modifier(prompt_modifier);
                if selected {
                    pad_style = pad_style.patch(theme.selection_bg);
                }

                spans.push(Span::styled(
                    " ".repeat(content_width.saturating_sub(used_width)),
                    pad_style,
                ));
            }
            rendered.push(Line::from(spans));
        }
        return rendered;
    }

    let mut spans = build_row_spans(
        &line,
        line_status,
        cursor_line,
        selected,
        now_millis,
        theme,
        true,
    );

    let used_width = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    if used_width < content_width {
        let prompt_modifier =
            if line_status == Some(TranscriptLineStatus::Prompt(PromptStatus::Cancelled)) {
                theme.cancelled_modifier
            } else {
                Modifier::empty()
            };

        let mut pad_style = transcript_row_style(line.role, theme).add_modifier(prompt_modifier);
        if selected {
            pad_style = pad_style.patch(theme.selection_bg);
        }

        spans.push(Span::styled(
            " ".repeat(content_width.saturating_sub(used_width)),
            pad_style,
        ));
    }

    vec![Line::from(spans)]
}

pub(super) fn prompt_indicator_for_status(status: PromptStatus, now_millis: u128) -> &'static str {
    match status {
        PromptStatus::Queued => "•",
        PromptStatus::InProgress => {
            let idx = ((now_millis / 100) % super::IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
            super::IN_PROGRESS_SPINNER_FRAMES[idx]
        }
        PromptStatus::Done => "✓",
        PromptStatus::Cancelled => "✕",
    }
}

pub(super) fn tool_indicator_for_status(status: ToolCallStatus, now_millis: u128) -> &'static str {
    match status {
        ToolCallStatus::InProgress => {
            let idx = ((now_millis / 100) % super::IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
            super::IN_PROGRESS_SPINNER_FRAMES[idx]
        }
        ToolCallStatus::Done => "✓",
        ToolCallStatus::Failed => "✕",
    }
}

pub(super) fn indicator_for_line_status(status: TranscriptLineStatus, now_millis: u128) -> &'static str {
    match status {
        TranscriptLineStatus::Prompt(prompt) => prompt_indicator_for_status(prompt, now_millis),
        TranscriptLineStatus::Tool(tool) => tool_indicator_for_status(tool, now_millis),
    }
}

pub(super) fn indicator_style_for_status(status: TranscriptLineStatus, theme: &TuiTheme) -> Style {
    match status {
        TranscriptLineStatus::Prompt(prompt_status) => match prompt_status {
            PromptStatus::Queued => theme.status_queued,
            PromptStatus::InProgress => theme.status_running,
            PromptStatus::Done => theme.status_done,
            PromptStatus::Cancelled => theme.status_cancelled,
        },
        TranscriptLineStatus::Tool(tool_status) => match tool_status {
            ToolCallStatus::InProgress => theme.status_running,
            ToolCallStatus::Done => theme.status_done,
            ToolCallStatus::Failed => theme.status_failed,
        },
    }
}

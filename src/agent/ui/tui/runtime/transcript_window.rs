use crate::agent::ui::tui::state::{TranscriptLine, TranscriptLineStatus, TranscriptRole};
use crate::agent::ui::tui::{rendering::theme::TuiTheme, runtime::render_transcript_lines};

fn wrapped_visual_rows_for_rendered_line(
    rendered_line: &ratatui::text::Line<'_>,
    content_width: usize,
) -> usize {
    let width = rendered_line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>()
        .max(1);
    width.div_ceil(content_width.max(1))
}

#[cfg(test)]
pub(in crate::agent::ui::tui) fn visible_transcript_window(
    transcript: &[TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
) -> Vec<TranscriptLine> {
    let (_, lines) = visible_transcript_window_with_start(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
    );
    lines
}

#[cfg(test)]
fn visible_transcript_window_with_start(
    transcript: &[TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
) -> (usize, Vec<TranscriptLine>) {
    let total_lines = transcript.len();
    if visible_lines == 0 || total_lines == 0 {
        return (0, Vec::new());
    }

    if total_lines <= visible_lines {
        return (0, transcript.to_vec());
    }

    let max_from_bottom = total_lines - visible_lines;
    let from_bottom = if follow_tail {
        0
    } else {
        scroll_from_bottom.min(max_from_bottom)
    };

    let start = total_lines - visible_lines - from_bottom;
    let end = (start + visible_lines).min(total_lines);
    (start, transcript[start..end].to_vec())
}

pub(super) fn visible_transcript_window_for_render_with_required_line(
    transcript: &[TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
    required_line_index: Option<usize>,
    line_statuses: &[Option<TranscriptLineStatus>],
) -> (usize, Vec<TranscriptLine>) {
    let total_lines = transcript.len();
    if total_lines == 0 || visible_lines == 0 {
        return (0, Vec::new());
    }

    let max_scroll_from_bottom = total_lines.saturating_sub(visible_lines);
    let end_exclusive = if follow_tail {
        total_lines
    } else {
        total_lines.saturating_sub(scroll_from_bottom.min(max_scroll_from_bottom))
    }
    .clamp(1, total_lines);

    let preferred = fit_window_ending_at_by_wrapped_rows(
        transcript,
        end_exclusive,
        visible_lines,
        content_width,
        line_statuses,
    );

    let Some(required) = required_line_index.filter(|idx| *idx < total_lines) else {
        return preferred;
    };

    if window_contains_line(&preferred, required) {
        return preferred;
    }

    let required_end = required.saturating_add(1).clamp(1, total_lines);
    fit_window_ending_at_by_wrapped_rows(
        transcript,
        required_end,
        visible_lines,
        content_width,
        line_statuses,
    )
}

fn window_contains_line(window: &(usize, Vec<TranscriptLine>), line_index: usize) -> bool {
    let start = window.0;
    let end = start.saturating_add(window.1.len());
    line_index >= start && line_index < end
}

fn fit_window_ending_at_by_wrapped_rows(
    transcript: &[TranscriptLine],
    end_exclusive: usize,
    visible_lines: usize,
    content_width: usize,
    line_statuses: &[Option<TranscriptLineStatus>],
) -> (usize, Vec<TranscriptLine>) {
    if end_exclusive == 0 || visible_lines == 0 {
        return (0, Vec::new());
    }

    let width = content_width.max(1);
    let mut start = end_exclusive;
    let mut used_rows = 0usize;
    let mut next_included_role: Option<TranscriptRole> = None;

    for idx in (0..end_exclusive).rev() {
        let line = &transcript[idx];
        let line_status = line_statuses.get(idx).copied().flatten();
        let rows = rendered_row_count_for_line(line, width, line_status);
        let spacer_rows = if let Some(next_role) = next_included_role {
            if should_insert_transition_spacer(Some(line.role), next_role) {
                1
            } else {
                0
            }
        } else {
            0
        };

        if used_rows.saturating_add(spacer_rows).saturating_add(rows) > visible_lines {
            if start == end_exclusive {
                start = idx;
            }
            break;
        }

        used_rows = used_rows.saturating_add(spacer_rows).saturating_add(rows);
        start = idx;
        next_included_role = Some(line.role);
    }

    if start == end_exclusive {
        start = end_exclusive.saturating_sub(1);
    }

    (start, transcript[start..end_exclusive].to_vec())
}

fn rendered_row_count_for_line(
    line: &TranscriptLine,
    content_width: usize,
    line_status: Option<TranscriptLineStatus>,
) -> usize {
    render_transcript_lines(
        line.clone(),
        content_width,
        false,
        false,
        line_status,
        0,
        &TuiTheme::default(),
    )
    .iter()
    .map(|rendered_line| wrapped_visual_rows_for_rendered_line(rendered_line, content_width))
    .sum::<usize>()
    .max(1)
}

pub(super) fn should_insert_transition_spacer(
    previous: Option<TranscriptRole>,
    next: TranscriptRole,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };

    if previous == next {
        return false;
    }

    if previous == TranscriptRole::Separator || next == TranscriptRole::Separator {
        return false;
    }

    !is_user_assistant_transition(previous, next)
}

fn is_user_assistant_transition(previous: TranscriptRole, next: TranscriptRole) -> bool {
    matches!(
        (previous, next),
        (TranscriptRole::User, TranscriptRole::Assistant)
            | (TranscriptRole::Assistant, TranscriptRole::User)
            | (TranscriptRole::Tool, TranscriptRole::ToolDisplay)
            | (TranscriptRole::ToolDisplay, TranscriptRole::Tool)
    )
}

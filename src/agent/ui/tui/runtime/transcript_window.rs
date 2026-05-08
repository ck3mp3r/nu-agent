use crate::agent::ui::tui::state::{TranscriptLine, TranscriptRole};
use crate::agent::ui::tui::{
    rendering::theme::TuiTheme,
    runtime::render_transcript_lines,
};

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

pub(super) fn visible_transcript_window_for_render(
    transcript: &[TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
) -> (usize, Vec<TranscriptLine>) {
    if !follow_tail {
        return visible_transcript_window_with_start(
            transcript,
            visible_lines,
            scroll_from_bottom,
            follow_tail,
        );
    }

    fit_tail_window_by_wrapped_rows(transcript, visible_lines, content_width)
}

fn fit_tail_window_by_wrapped_rows(
    transcript: &[TranscriptLine],
    visible_lines: usize,
    content_width: usize,
) -> (usize, Vec<TranscriptLine>) {
    let total_lines = transcript.len();
    if total_lines == 0 || visible_lines == 0 {
        return (0, Vec::new());
    }

    let width = content_width.max(1);
    let mut start = total_lines;
    let mut used_rows = 0usize;
    let mut next_included_role: Option<TranscriptRole> = None;

    for idx in (0..total_lines).rev() {
        let line = &transcript[idx];
        let rows = rendered_row_count_for_line(line, width);
        let spacer_rows = if let Some(next_role) = next_included_role {
            if should_insert_transition_spacer(Some(line.role), next_role) {
                1
            } else {
                0
            }
        } else {
            0
        };

        if used_rows
            .saturating_add(spacer_rows)
            .saturating_add(rows)
            > visible_lines
        {
            if start == total_lines {
                start = idx;
            }
            break;
        }

        used_rows = used_rows.saturating_add(spacer_rows).saturating_add(rows);
        start = idx;
        next_included_role = Some(line.role);
    }

    if start == total_lines {
        start = total_lines.saturating_sub(1);
    }

    (start, transcript[start..total_lines].to_vec())
}

fn rendered_row_count_for_line(line: &TranscriptLine, content_width: usize) -> usize {
    render_transcript_lines(line.clone(), content_width, false, false, None, 0, &TuiTheme::default())
        .iter()
        .map(|rendered_line| wrapped_visual_rows_for_rendered_line(rendered_line, content_width))
        .sum::<usize>()
        .max(1)
}

pub(super) fn should_insert_transition_spacer(previous: Option<TranscriptRole>, next: TranscriptRole) -> bool {
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
    )
}

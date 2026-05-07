use super::{
    cancel::CancelController,
    state::{AppState, InputMode, PaneFocus, TranscriptRole, UiPhase},
};
use crate::commands::agent::ui::event::UiEvent;

pub const ESC_ABORT_CONFIRM_STATUS: &str = "Hit escape again to abort.";
const ABORT_REQUESTED_STATUS: &str = "Abort requested.";
const ABORT_REQUESTED_MARKER: &str = "[abort requested]";
const VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS: &str =
    "Visual mode requires transcript focus (Tab/h/l).";
const TRANSCRIPT_PAGE_LINES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAction {
    InsertChar(char),
    InsertNewline,
    Submit,
    Backspace,
    Delete,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    HistoryUp,
    HistoryDown,
    ScrollPageUp,
    ScrollPageDown,
    CompleteForward,
    CompleteBackward,
    Resize { columns: u16, rows: u16 },
    Quit,
    Esc,
    EscConfirm,
    EnterInsertMode,
    EnterNormalModeFromChord,
    ScrollLineUp,
    ScrollLineDown,
    ScrollToTop,
    ScrollToBottom,
    FocusPaneLeft,
    FocusPaneRight,
    EnterVisualMode,
    YankSelection,
    Noop,
}

#[derive(Debug, Clone)]
pub enum ReducerInput {
    User(UserAction),
    Event(UiEvent),
}

pub fn reduce_with_cancel_controller(
    state: &mut AppState,
    input: ReducerInput,
    cancel_controller: Option<&CancelController>,
) {
    match input {
        ReducerInput::User(action) => reduce_user_action(state, action, cancel_controller),
        ReducerInput::Event(event) => reduce_ui_event(state, event),
    }
}

fn reduce_user_action(
    state: &mut AppState,
    action: UserAction,
    cancel_controller: Option<&CancelController>,
) {
    match action {
        UserAction::InsertChar(ch) => {
            state.append_input_char(ch);
        }
        UserAction::InsertNewline => {
            state.insert_input_newline();
        }
        UserAction::Backspace => {
            state.backspace_input_char();
        }
        UserAction::Delete => {
            state.delete_input_char();
        }
        UserAction::Submit => {
            let submitted_text = state.input.buffer.clone();
            if submitted_text.trim().is_empty() {
                return;
            }
            state.enqueue_prompt(submitted_text);
            state.input.buffer.clear();
            state.input.cursor = 0;
        }
        UserAction::MoveCursorLeft => state.move_cursor_left(),
        UserAction::MoveCursorRight => state.move_cursor_right(),
        UserAction::MoveCursorHome => state.move_cursor_home(),
        UserAction::MoveCursorEnd => state.move_cursor_end(),
        UserAction::Noop => {}
        UserAction::EnterInsertMode => {
            if state.phase == UiPhase::Idle {
                state.enter_insert_mode();
            }
        }
        UserAction::EnterVisualMode => {
            if state.phase == UiPhase::Idle {
                if state.pane_focus == PaneFocus::Transcript {
                    state.enter_visual_mode();
                } else {
                    state.status_line = VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS.to_string();
                }
            }
        }
        UserAction::EnterNormalModeFromChord => {
            if state.phase == UiPhase::Idle {
                state.backspace_input_char();
                state.enter_normal_mode();
            }
        }
        UserAction::ScrollLineUp => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_line_up();
            } else {
                state.scroll_transcript_line_up();
            }
        }
        UserAction::ScrollLineDown => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_line_down();
            } else {
                state.scroll_transcript_line_down();
            }
        }
        UserAction::ScrollToTop => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_to_top();
            } else {
                state.scroll_transcript_to_top();
            }
        }
        UserAction::ScrollToBottom => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_to_bottom();
            } else {
                state.scroll_transcript_to_bottom();
            }
        }
        UserAction::FocusPaneLeft => {
            state.focus_prev_pane();
        }
        UserAction::FocusPaneRight => {
            state.focus_next_pane();
        }
        UserAction::YankSelection => {
            if state.input_mode == InputMode::Visual {
                state.queue_visual_selection_to_clipboard();
                state.enter_normal_mode();
            }
        }
        UserAction::Resize { rows, .. } => {
            let header = 1usize;
            let status = 6usize;
            let input = 2usize;
            let borders = 2usize;
            let transcript_lines = usize::from(rows)
                .saturating_sub(header)
                .saturating_sub(status)
                .saturating_sub(input)
                .saturating_sub(borders)
                .max(1);
            state.set_transcript_viewport_lines(transcript_lines);
        }
        UserAction::HistoryUp
        | UserAction::HistoryDown
        | UserAction::CompleteForward
        | UserAction::CompleteBackward => {}
        UserAction::ScrollPageUp => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_page_up(TRANSCRIPT_PAGE_LINES);
            } else {
                state.scroll_transcript_page_up(TRANSCRIPT_PAGE_LINES);
            }
        }
        UserAction::ScrollPageDown => {
            if state.input_mode == InputMode::Visual {
                state.extend_visual_cursor_page_down(TRANSCRIPT_PAGE_LINES);
            } else {
                state.scroll_transcript_page_down(TRANSCRIPT_PAGE_LINES);
            }
        }
        UserAction::Quit => {
            state.request_quit_if_idle();
        }
        UserAction::Esc => {
            if state.phase == UiPhase::Idle && state.input_mode == InputMode::Insert {
                state.enter_normal_mode();
                return;
            }
            if state.phase == UiPhase::Idle && state.input_mode == InputMode::Visual {
                state.enter_normal_mode();
                return;
            }
            if state.request_abort_confirmation() {
                state.status_line = ESC_ABORT_CONFIRM_STATUS.to_string();
            }
        }
        UserAction::EscConfirm => {
            if state.phase == UiPhase::AbortPending && state.abort.pending && state.is_active_cycle() {
                if let Some(controller) = cancel_controller {
                    controller.request_cancel();
                }
                state.cancel_active_and_pending_prompts();
                state.status_line = ABORT_REQUESTED_STATUS.to_string();
                state.push_transcript_line(
                    TranscriptRole::System,
                    ABORT_REQUESTED_MARKER.to_string(),
                );
            }
        }
    }
}

fn reduce_ui_event(state: &mut AppState, event: UiEvent) {
    match event {
        UiEvent::LlmStart => {
            if state.phase == UiPhase::Idle {
                state.phase = UiPhase::Busy;
                state.ensure_invariants();
            }
        }
        UiEvent::Tick => {
            if state.status_line.is_empty() {
                state.status_line = "Thinking...".to_string();
            }
        }
        UiEvent::ToolStart {
            name, arguments, ..
        } => {
            state.start_tool_call(&name, &arguments);
            state.status_line = format!("Tool: {name}");
        }
        UiEvent::ToolEnd {
            name,
            arguments,
            success,
            ..
        } => {
            state.finish_tool_call(&name, &arguments, success);
            state.status_line = "Thinking...".to_string();
        }
        UiEvent::LlmEnd {
            response_chars,
            input_tokens,
            output_tokens,
            total_tokens,
            ..
        } => {
            state.record_token_usage(input_tokens, output_tokens, total_tokens);
            state.status_line = format!("Response ready ({response_chars} chars)");
        }
        UiEvent::Warning { message } => {
            state.status_line = message;
        }
        UiEvent::AssistantMessage { text } => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                state.scroll_transcript_to_bottom();
                for line in state.project_assistant_markdown_lines(trimmed) {
                    let text = super::markdown::rendered_line_to_plain_text(&line);
                    if text.trim().is_empty() {
                        continue;
                    }
                    state.push_transcript_rendered_line(TranscriptRole::Assistant, line);
                }
            }
        }
        UiEvent::Completed { .. } => {
            finalize(state);
        }
    }
}

fn finalize(state: &mut AppState) {
    state.finalize_cycle();
    state.status_line.clear();
}

pub(crate) fn summarize_tool_arguments(arguments: &str) -> String {
    const MAX_LEN: usize = 120;
    let compact = arguments
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= MAX_LEN {
        return compact;
    }

    let mut truncated = compact.chars().take(MAX_LEN).collect::<String>();
    truncated.push('…');
    truncated
}

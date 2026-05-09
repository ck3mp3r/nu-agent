use crate::agent::ui::tui::{
    interaction::cancel::CancelController,
    markdown,
    state::{AppState, InputMode, PaneFocus, TranscriptRole, UiPhase},
};
use crate::agent::protocol::event::UiEvent;

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
    ToggleCommandPalette,
    CommandPaletteMoveUp,
    CommandPaletteMoveDown,
    CommandPaletteSelect,
    CommandPaletteClose,
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
        UserAction::InsertChar(ch) => handle_insert_char(state, ch),
        UserAction::InsertNewline => handle_insert_newline(state),
        UserAction::Backspace => handle_backspace(state),
        UserAction::Delete => handle_delete(state),
        UserAction::Submit => handle_submit(state),
        UserAction::MoveCursorLeft => handle_move_cursor_left(state),
        UserAction::MoveCursorRight => handle_move_cursor_right(state),
        UserAction::MoveCursorHome => handle_move_cursor_home(state),
        UserAction::MoveCursorEnd => handle_move_cursor_end(state),
        UserAction::Noop => {}
        UserAction::EnterInsertMode => handle_enter_insert_mode(state),
        UserAction::EnterVisualMode => handle_enter_visual_mode(state),
        UserAction::EnterNormalModeFromChord => handle_enter_normal_mode_from_chord(state),
        UserAction::ScrollLineUp => handle_scroll_line_up(state),
        UserAction::ScrollLineDown => handle_scroll_line_down(state),
        UserAction::ScrollToTop => handle_scroll_to_top(state),
        UserAction::ScrollToBottom => handle_scroll_to_bottom(state),
        UserAction::FocusPaneLeft => handle_focus_pane_left(state),
        UserAction::FocusPaneRight => handle_focus_pane_right(state),
        UserAction::YankSelection => handle_yank_selection(state),
        UserAction::Resize { rows, .. } => handle_resize(state, rows),
        UserAction::ToggleCommandPalette => handle_toggle_command_palette(state),
        UserAction::CommandPaletteMoveUp => state.command_palette_move_up(),
        UserAction::CommandPaletteMoveDown => state.command_palette_move_down(),
        UserAction::CommandPaletteSelect => handle_command_palette_select(state),
        UserAction::CommandPaletteClose => state.close_command_palette(),
        UserAction::HistoryUp
        | UserAction::HistoryDown
        | UserAction::CompleteForward
        | UserAction::CompleteBackward => {}
        UserAction::ScrollPageUp => handle_scroll_page_up(state),
        UserAction::ScrollPageDown => handle_scroll_page_down(state),
        UserAction::Quit => handle_quit(state),
        UserAction::Esc => handle_escape(state),
        UserAction::EscConfirm => {
            handle_escape_confirm(state, cancel_controller);
        }
    }
}

fn handle_insert_char(state: &mut AppState, ch: char) {
    state.append_input_char(ch);
}

fn handle_insert_newline(state: &mut AppState) {
    state.insert_input_newline();
}

fn handle_backspace(state: &mut AppState) {
    state.backspace_input_char();
}

fn handle_delete(state: &mut AppState) {
    state.delete_input_char();
}

fn handle_submit(state: &mut AppState) {
    let submitted_text = state.input.buffer.clone();
    if submitted_text.trim().is_empty() {
        return;
    }
    state.enqueue_prompt(submitted_text);
    state.input.buffer.clear();
    state.input.cursor = 0;
}

fn handle_move_cursor_left(state: &mut AppState) {
    state.move_cursor_left();
}

fn handle_move_cursor_right(state: &mut AppState) {
    state.move_cursor_right();
}

fn handle_move_cursor_home(state: &mut AppState) {
    state.move_cursor_home();
}

fn handle_move_cursor_end(state: &mut AppState) {
    state.move_cursor_end();
}

fn handle_enter_insert_mode(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.enter_insert_mode();
    }
}

fn handle_enter_visual_mode(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        if state.pane_focus == PaneFocus::Transcript {
            state.enter_visual_mode();
        } else {
            state.status_line = VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS.to_string();
        }
    }
}

fn handle_enter_normal_mode_from_chord(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.backspace_input_char();
        state.enter_normal_mode();
    }
}

fn handle_scroll_line_up(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_line_up();
    } else {
        state.scroll_transcript_line_up();
    }
}

fn handle_scroll_line_down(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_line_down();
    } else {
        state.scroll_transcript_line_down();
    }
}

fn handle_scroll_to_top(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_to_top();
    } else {
        state.scroll_transcript_to_top();
    }
}

fn handle_scroll_to_bottom(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_to_bottom();
    } else {
        state.scroll_transcript_to_bottom();
    }
}

fn handle_focus_pane_left(state: &mut AppState) {
    state.focus_prev_pane();
}

fn handle_focus_pane_right(state: &mut AppState) {
    state.focus_next_pane();
}

fn handle_yank_selection(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.queue_visual_selection_to_clipboard();
        state.enter_normal_mode();
    }
}

fn handle_resize(state: &mut AppState, rows: u16) {
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

fn handle_toggle_command_palette(state: &mut AppState) {
    if state.command_palette_open {
        state.close_command_palette();
    } else {
        state.open_command_palette();
    }
}

fn handle_command_palette_select(state: &mut AppState) {
    if let Some(action) = state.command_palette_selected_action() {
        let panel = match action {
            crate::agent::ui::tui::state::CommandPaletteAction::Help => {
                crate::agent::ui::tui::state::InfoPanel::Help
            }
            crate::agent::ui::tui::state::CommandPaletteAction::Status => {
                crate::agent::ui::tui::state::InfoPanel::Status
            }
            crate::agent::ui::tui::state::CommandPaletteAction::Mcps => {
                crate::agent::ui::tui::state::InfoPanel::Mcps
            }
            crate::agent::ui::tui::state::CommandPaletteAction::Skills => {
                crate::agent::ui::tui::state::InfoPanel::Skills
            }
        };
        state.open_info_panel(panel);
    }
}

fn handle_scroll_page_up(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_page_up(TRANSCRIPT_PAGE_LINES);
    } else {
        state.scroll_transcript_page_up(TRANSCRIPT_PAGE_LINES);
    }
}

fn handle_scroll_page_down(state: &mut AppState) {
    if state.input_mode == InputMode::Visual {
        state.extend_visual_cursor_page_down(TRANSCRIPT_PAGE_LINES);
    } else {
        state.scroll_transcript_page_down(TRANSCRIPT_PAGE_LINES);
    }
}

fn handle_quit(state: &mut AppState) {
    state.request_quit_if_idle();
}

fn handle_escape(state: &mut AppState) {
    if state.info_panel.is_some() {
        state.close_info_panel();
        return;
    }

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

fn handle_escape_confirm(state: &mut AppState, cancel_controller: Option<&CancelController>) {
    if state.phase == UiPhase::AbortPending && state.abort.pending && state.is_active_cycle() {
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.cancel_active_and_pending_prompts();
        state.status_line = ABORT_REQUESTED_STATUS.to_string();
        state.push_transcript_line(TranscriptRole::System, ABORT_REQUESTED_MARKER.to_string());
    }
}

fn reduce_ui_event(state: &mut AppState, event: UiEvent) {
    match event {
        UiEvent::LlmStart => handle_llm_start(state),
        UiEvent::Tick => handle_tick(state),
        UiEvent::ToolStart {
            name, arguments, ..
        } => handle_tool_start(state, &name, &arguments),
        UiEvent::ToolEnd {
            name,
            arguments,
            success,
            ..
        } => handle_tool_end(state, &name, &arguments, success),
        UiEvent::LlmEnd {
            response_chars,
            input_tokens,
            output_tokens,
            total_tokens,
            ..
        } => handle_llm_end(state, response_chars, input_tokens, output_tokens, total_tokens),
        UiEvent::Warning { message } => handle_warning(state, message),
        UiEvent::AssistantMessage { text } => handle_assistant_message(state, text),
        UiEvent::Completed { .. } => finalize(state),
    }
}

fn handle_llm_start(state: &mut AppState) {
    if state.phase == UiPhase::Idle {
        state.phase = UiPhase::Busy;
        state.ensure_invariants();
    }
}

fn handle_tick(state: &mut AppState) {
    if state.status_line.is_empty() {
        state.status_line = "Thinking...".to_string();
    }
}

fn handle_tool_start(state: &mut AppState, name: &str, arguments: &str) {
    state.start_tool_call(name, arguments);
    state.status_line = format!("Tool: {name}");
}

fn handle_tool_end(state: &mut AppState, name: &str, arguments: &str, success: bool) {
    state.finish_tool_call(name, arguments, success);
    state.status_line = "Thinking...".to_string();
}

fn handle_llm_end(
    state: &mut AppState,
    response_chars: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
) {
    state.record_token_usage(input_tokens, output_tokens, total_tokens);
    state.status_line = format!("Response ready ({response_chars} chars)");
}

fn handle_warning(state: &mut AppState, message: String) {
    state.status_line = message;
}

fn handle_assistant_message(state: &mut AppState, text: String) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        if state.transcript_follow_tail {
            state.scroll_transcript_to_bottom();
        }
        for line in state.project_assistant_markdown_lines(trimmed) {
            let text = markdown::rendered_line_to_plain_text(&line);
            if text.trim().is_empty() {
                continue;
            }
            state.push_transcript_rendered_line(TranscriptRole::Assistant, line);
        }
    }
}

fn finalize(state: &mut AppState) {
    state.finalize_cycle();
    state.status_line.clear();
}

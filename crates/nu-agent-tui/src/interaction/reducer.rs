use crate::{
    interaction::cancel::CancelController,
    state::{
        ActivePicker, AppState, CommandPaletteAction, InputMode, PickerOption, PickerPayload,
        PickerRenderKind, ScrollAction, SubmitAction, SwitchRequest, TranscriptRole, UiPhase,
    },
};
use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::protocol::event::{PermissionDecision, PermissionRequestContext, UiEvent};
use nu_agent_core::protocol::slash::{SlashParseResult, extract_session_id, parse_slash_command};

pub(crate) const ESC_ABORT_CONFIRM_STATUS: &str = "Esc again to cancel";

pub(crate) const VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS: &str =
    "Visual mode requires transcript focus (Tab/h/l).";

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
    QueryNext,
    ToggleCommandPalette,
    PickerSubmit(SubmitAction),
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
    PermissionAllowOnce,
    PermissionAllowAlways,
    PermissionDeny,
    Noop,
}

#[derive(Debug, Clone)]
pub enum ReducerInput {
    User(UserAction),
    Event(Box<UiEvent>),
}

pub fn reduce_with_cancel_controller(
    state: &mut AppState,
    input: ReducerInput,
    cancel_controller: Option<&CancelController>,
) -> bool {
    match input {
        ReducerInput::User(action) => reduce_user_action(state, action, cancel_controller),
        ReducerInput::Event(event) => dispatch_ui_event(state, *event),
    }
}

fn reduce_user_action(
    state: &mut AppState,
    action: UserAction,
    cancel_controller: Option<&CancelController>,
) -> bool {
    // Selection-extension flag for scroll actions: derived from the input mode
    // (input domain) and carried into the scroll action.
    let select = state.input.mode == InputMode::Visual;
    match action {
        UserAction::InsertNewline => handle_insert_newline(state),
        UserAction::Submit => handle_submit(state),
        // Input editing is handled by the coordinator's TextArea; these
        // actions are no-ops at the reducer boundary.
        UserAction::InsertChar(_) | UserAction::Backspace | UserAction::Delete => false,
        UserAction::MoveCursorLeft
        | UserAction::MoveCursorRight
        | UserAction::MoveCursorHome
        | UserAction::MoveCursorEnd => false,
        UserAction::Noop => false,
        UserAction::EnterInsertMode => handle_enter_insert_mode(state),
        UserAction::EnterVisualMode => handle_enter_visual_mode(state),
        UserAction::EnterNormalModeFromChord => handle_enter_normal_mode_from_chord(state),
        UserAction::ScrollLineUp => state
            .scroll
            .reduce_scroll_action(ScrollAction::LineUp { select }),
        UserAction::ScrollLineDown => state
            .scroll
            .reduce_scroll_action(ScrollAction::LineDown { select }),
        UserAction::ScrollToTop => state
            .scroll
            .reduce_scroll_action(ScrollAction::ToTop { select }),
        UserAction::ScrollToBottom => state
            .scroll
            .reduce_scroll_action(ScrollAction::ToBottom { select }),
        UserAction::FocusPaneLeft => state
            .scroll
            .reduce_scroll_action(ScrollAction::FocusPaneLeft),
        UserAction::FocusPaneRight => state
            .scroll
            .reduce_scroll_action(ScrollAction::FocusPaneRight),
        UserAction::YankSelection => handle_yank_selection(state),
        UserAction::PermissionAllowOnce => {
            if state
                .permission
                .submit_decision(PermissionDecision::AllowOnce)
            {
                state.ensure_invariants();
            }
            true
        }
        UserAction::PermissionAllowAlways => {
            if state
                .permission
                .submit_decision(PermissionDecision::AllowAlways)
            {
                state.ensure_invariants();
            }
            true
        }
        UserAction::PermissionDeny => {
            if state.permission.submit_decision(PermissionDecision::Deny) {
                state.ensure_invariants();
            }
            true
        }
        UserAction::Resize { .. } => false,
        UserAction::ToggleCommandPalette => handle_toggle_command_palette(state),
        UserAction::PickerSubmit(submit) => handle_picker_submit(state, submit),
        UserAction::HistoryUp => false,
        UserAction::HistoryDown => false,
        UserAction::QueryNext | UserAction::CompleteForward | UserAction::CompleteBackward => false,
        UserAction::ScrollPageUp => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageUp { select }),
        UserAction::ScrollPageDown => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageDown { select }),
        UserAction::Quit => handle_quit(state, cancel_controller),
        UserAction::Esc => handle_escape(state),
        UserAction::EscConfirm => handle_escape_confirm(state, cancel_controller),
    }
}

fn handle_insert_newline(state: &mut AppState) -> bool {
    let _ = state;
    false
}

fn handle_submit(state: &mut AppState) -> bool {
    // handle_submit is only called from non-insert-mode paths (normal mode, etc.)
    // where there's no textarea. The caller must set pending_submit_text on
    // state.input before dispatching Submit.
    let submitted_text = state.input.pending_submit_text.take().unwrap_or_default();
    if submitted_text.trim().is_empty() {
        return false;
    }

    match parse_slash_command(&submitted_text) {
        SlashParseResult::Command(nu_agent_core::protocol::slash::SlashCommand::Models) => {
            state.queue_launch_request(SharedUiAction::Models);
            return true;
        }
        SlashParseResult::Command(nu_agent_core::protocol::slash::SlashCommand::Agent) => {
            state.queue_launch_request(SharedUiAction::Agents);
            return true;
        }
        SlashParseResult::Command(nu_agent_core::protocol::slash::SlashCommand::Session) => {
            if let Some(session_id) = extract_session_id(&submitted_text) {
                state.queue_switch_request(SwitchRequest::Session(session_id.to_string()));
            } else {
                state.queue_launch_request(SharedUiAction::Sessions);
            }
            return true;
        }
        SlashParseResult::Command(nu_agent_core::protocol::slash::SlashCommand::Theme) => {
            state.queue_launch_request(SharedUiAction::Themes);
            return true;
        }
        SlashParseResult::Command(_) | SlashParseResult::Unknown(_) => {
            state.enqueue_immediate_submission(submitted_text);
            return true;
        }
        SlashParseResult::NotSlash => {}
    }

    state.enqueue_prompt(submitted_text);
    true
}

fn handle_enter_insert_mode(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_insert_mode();
        return true;
    }
    false
}

fn handle_enter_visual_mode(state: &mut AppState) -> bool {
    if state.phase != UiPhase::Idle {
        return false;
    }
    // Scroll domain: focus guard, selection start, status lines. The input
    // mode transition stays here (input domain).
    let entered = state.scroll.enter_visual_mode(&mut state.status);
    if entered {
        state.input.mode = InputMode::Visual;
    }
    true
}

fn handle_enter_normal_mode_from_chord(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_normal_mode();
        return true;
    }
    false
}

fn handle_yank_selection(state: &mut AppState) -> bool {
    if state.input.mode != InputMode::Visual {
        return false;
    }
    // Scroll domain: payload extraction from the rendered viewport. The
    // clipboard request stays here (input domain).
    if state.scroll.selection.is_some() {
        let payload = state.scroll.yank_selection();
        if let Some(payload) = payload {
            state.input.set_clipboard_request(payload);
        }
    }
    state.enter_normal_mode();
    true
}

fn handle_toggle_command_palette(state: &mut AppState) -> bool {
    if state.picker.render_kind() == Some(PickerRenderKind::CommandPalette) {
        state.picker.close();
    } else {
        state.info_panel = None;
        let entry = state.picker.open(ActivePicker::CommandPalette);
        entry.state.options = CommandPaletteAction::PALETTE_ACTIONS
            .iter()
            .map(|a| PickerOption {
                id: a.label().to_string(),
                display: a.label().to_string(),
                search_text: a.label().to_string(),
                payload: PickerPayload::Command(*a),
            })
            .collect();
    }
    true
}

fn handle_picker_submit(state: &mut AppState, submit: SubmitAction) -> bool {
    match submit {
        SubmitAction::Switch(req) => {
            state.queue_switch_request(req);
            state.picker.close();
            true
        }
        SubmitAction::Launch(kind) => {
            let action = match kind {
                ActivePicker::Model => SharedUiAction::Models,
                ActivePicker::Agent => SharedUiAction::Agents,
                ActivePicker::Session => SharedUiAction::Sessions,
                ActivePicker::Theme => SharedUiAction::Themes,
                _ => return false,
            };
            state.picker.close();
            state.queue_launch_request(action);
            true
        }
        SubmitAction::Command(action) => {
            if action == crate::state::CommandPaletteAction::Models {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Models);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Agents {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Agents);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Sessions {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Sessions);
                return true;
            }
            if action == crate::state::CommandPaletteAction::Theme {
                state.picker.close();
                state.queue_launch_request(SharedUiAction::Themes);
                return true;
            }
            if let Some(panel) = action.info_panel() {
                state.open_info_panel(panel);
            } else {
                state.picker.close();
            }
            true
        }
        SubmitAction::SlashAccept => {
            let Some(opt) = state.picker.active_state().and_then(|s| s.selected()) else {
                return false;
            };
            let command = match &opt.payload {
                PickerPayload::Slash(c) => *c,
                _ => return false,
            };
            let selected = command.label().to_string();
            state.input.pending_submit_text = Some(selected);
            state.ensure_invariants();
            handle_submit(state);
            true
        }
    }
}

fn handle_quit(state: &mut AppState, cancel_controller: Option<&CancelController>) -> bool {
    if state.phase != crate::state::UiPhase::Idle {
        // Busy: cancel the running turn and quit
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.quit_requested = true;
    } else {
        // Idle: quit
        state.request_quit_if_idle();
    }
    true
}

fn handle_escape(state: &mut AppState) -> bool {
    if state.permission.has_prompt() {
        if state.permission.submit_decision(PermissionDecision::Deny) {
            state.ensure_invariants();
        }
        return true;
    }

    if state.info_panel.is_some() {
        state.close_info_panel();
        return true;
    }

    if state.phase == UiPhase::Idle && state.input.mode == InputMode::Insert {
        state.enter_normal_mode();
        return true;
    }
    if state.phase == UiPhase::Idle && state.input.mode == InputMode::Visual {
        state.scroll.selection = None;
        state.enter_normal_mode();
        return true;
    }
    if state.request_abort_confirmation() {
        state.status.message.set_message(ESC_ABORT_CONFIRM_STATUS);
        return true;
    }
    false
}

fn handle_escape_confirm(
    state: &mut AppState,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if state.phase == UiPhase::AbortPending && state.abort.pending && state.is_active_cycle() {
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.cancel_and_restore_pending_to_input();
        state.transcript.push_spacer();
        state.status.message.clear();
        return true;
    }
    false
}

/// Dispatch a protocol `UiEvent` to the domain reducers. This is the single
/// `UiEvent` → domain mapping; both event paths (bus receivers via
/// `RuntimeCoordinator::reduce_*_event` and the transport drain) converge
/// here or on the same domain reducers.
pub(crate) fn dispatch_ui_event(state: &mut AppState, event: UiEvent) -> bool {
    use nu_agent_core::bus::{CompactionEvent, LlmEvent, ToolEvent, TurnEvent};

    match event {
        UiEvent::LlmStarted => crate::state::dispatch_llm_event(state, LlmEvent::Started),
        UiEvent::LlmCompleted {
            response_chars,
            tool_calls,
            input_tokens,
            output_tokens,
            total_tokens,
        } => crate::state::dispatch_llm_event(
            state,
            LlmEvent::Completed {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
            },
        ),
        UiEvent::AssistantMessage { text } => {
            crate::state::dispatch_llm_event(state, LlmEvent::AssistantMessage { text })
        }
        UiEvent::ToolStarted {
            name,
            source,
            arguments,
        } => crate::state::dispatch_tool_event(
            state,
            ToolEvent::Started {
                name,
                source,
                arguments,
            },
        ),
        UiEvent::ToolCompleted {
            name,
            source,
            arguments,
            success,
            result,
            display,
            error_kind,
            message,
        } => crate::state::dispatch_tool_event(
            state,
            ToolEvent::Completed {
                name,
                source,
                arguments,
                success,
                result,
                display,
                error_kind,
                message,
            },
        ),
        UiEvent::CompactionStarted { source } => {
            crate::state::dispatch_compaction_event(state, CompactionEvent::Started { source })
        }
        UiEvent::CompactionSummaryChunk {
            source,
            delta,
            aggregated,
        } => crate::state::dispatch_compaction_event(
            state,
            CompactionEvent::SummaryChunk {
                source,
                delta,
                aggregated,
            },
        ),
        UiEvent::CompactionCompleted {
            source,
            summary_preview,
            summary_body,
        } => crate::state::dispatch_compaction_event(
            state,
            CompactionEvent::Completed {
                source,
                summary_preview,
                summary_body,
            },
        ),
        UiEvent::CompactionFailed { source, message } => crate::state::dispatch_compaction_event(
            state,
            CompactionEvent::Failed { source, message },
        ),
        UiEvent::Completed { tool_calls } => {
            crate::state::dispatch_turn_event(state, TurnEvent::Completed { tool_calls })
        }
        UiEvent::Tick => true,
        UiEvent::TurnError { message } => {
            if !state.transcript.last_is_spacer() && !state.transcript.is_empty() {
                state.transcript.push_spacer();
            }
            state.transcript.push_spacer();
            state
                .transcript
                .push_transcript_line(TranscriptRole::System, format!("Error: {message}"));
            crate::state::dispatch_turn_event(state, TurnEvent::Completed { tool_calls: 0 });
            true
        }
        UiEvent::PermissionRequested { .. }
        | UiEvent::PermissionDecisionSubmitted { .. }
        | UiEvent::PermissionDecisionTimedOut { .. }
        | UiEvent::PermissionDecisionIgnored { .. }
        | UiEvent::Warning { .. } => false,
    }
}

pub(crate) fn apply_permission_request_display(
    state: &mut AppState,
    context: &PermissionRequestContext,
) {
    crate::state::note_permission_request_display(&mut state.tool, &mut state.transcript, context);
}

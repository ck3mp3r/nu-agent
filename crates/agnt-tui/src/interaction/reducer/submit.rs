//! The submit lanes: one typed submission, and the picker's accept.

use crate::state::{ActivePicker, AppState, PromptFacts, SubmitAction, SwitchRequest};
use agnt_runtime::protocol::contracts::SharedUiAction;
use agnt_runtime::protocol::slash::{SlashParseResult, extract_session_id, parse_slash_command};

pub(super) fn handle_submit(
    state: &mut AppState,
    submitted_text: String,
    facts: PromptFacts,
) -> bool {
    state.input.pending_submit_text = None;
    if submitted_text.trim().is_empty() {
        return false;
    }

    match parse_slash_command(&submitted_text) {
        SlashParseResult::Command(agnt_runtime::protocol::slash::SlashCommand::Models) => {
            state.queue_launch_request(SharedUiAction::Models);
            return true;
        }
        SlashParseResult::Command(agnt_runtime::protocol::slash::SlashCommand::Agent) => {
            state.queue_launch_request(SharedUiAction::Agents);
            return true;
        }
        SlashParseResult::Command(agnt_runtime::protocol::slash::SlashCommand::Session) => {
            if let Some(session_id) = extract_session_id(&submitted_text) {
                state.queue_switch_request(SwitchRequest::Session(session_id.to_string()));
            } else {
                state.queue_launch_request(SharedUiAction::Sessions);
            }
            return true;
        }
        SlashParseResult::Command(agnt_runtime::protocol::slash::SlashCommand::Theme) => {
            state.queue_launch_request(SharedUiAction::Themes);
            return true;
        }
        SlashParseResult::Command(_) | SlashParseResult::Unknown(_) => {
            state.enqueue_immediate_submission_with_facts(submitted_text, facts);
            return true;
        }
        SlashParseResult::NotSlash => {}
    }

    state.enqueue_prompt_with_facts(submitted_text, facts);
    true
}

pub(super) fn handle_picker_submit(state: &mut AppState, submit: SubmitAction) -> bool {
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
            let Some(selected) = state.picker.selected_slash_label() else {
                return false;
            };
            state.picker.close();
            let facts = state.submit_facts();
            handle_submit(state, selected, facts)
        }
    }
}

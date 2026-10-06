//! Shared fixtures for the picker state test modules.
//!
//! Holds the imports and the helper functions every topical picker test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::dispatch::dispatch_terminal_event;
pub(super) use crate::interaction::input::{TerminalEvent, TerminalKey};
pub(super) use crate::state::*;
pub(super) use crate::test_support::open_command_palette_for_test;
pub(super) use nu_agent_core::protocol::contracts::SharedUiAction;
pub(super) use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

pub(super) fn palette_ids(state: &AppState) -> Vec<String> {
    state
        .picker
        .active_state()
        .unwrap()
        .filtered()
        .iter()
        .map(|o| o.id.clone())
        .collect()
}

pub(super) fn test_model_options() -> Vec<ModelPickerOption> {
    vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
        },
    ]
}

pub(super) fn test_agent_options() -> Vec<AgentPickerOption> {
    vec![
        AgentPickerOption {
            name: "alpha".into(),
            description: Some("Alpha agent".into()),
            display: "alpha — Alpha agent".into(),
            builtin: false,
        },
        AgentPickerOption {
            name: "beta".into(),
            description: None,
            display: "beta".into(),
            builtin: false,
        },
        AgentPickerOption {
            name: "gamma".into(),
            description: Some("Gamma agent".into()),
            display: "gamma — Gamma agent".into(),
            builtin: false,
        },
    ]
}

pub(super) fn test_session_options() -> Vec<PickerOption> {
    let now = chrono::Utc::now();
    vec![
        PickerOption {
            id: "old".to_string(),
            display: "old".to_string(),
            search_text: "old".to_string(),
            sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(
                now - chrono::Duration::days(1),
            ))],
            payload: PickerPayload::Session {
                session_id: "old".to_string(),
                title: None,
                created_at: now - chrono::Duration::days(1),
                message_count: 1,
            },
        },
        PickerOption {
            id: "new".to_string(),
            display: "new".to_string(),
            search_text: "new".to_string(),
            sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(now))],
            payload: PickerPayload::Session {
                session_id: "new".to_string(),
                title: None,
                created_at: now,
                message_count: 1,
            },
        },
    ]
}

pub(super) fn type_query(state: &mut AppState, query: &str) -> Result<()> {
    for ch in query.chars() {
        state
            .picker
            .active_state_mut()
            .ok_or("picker should be open")?
            .append_query_char(ch);
    }
    Ok(())
}

pub(super) fn picker_option(id: &str) -> PickerOption {
    PickerOption {
        id: id.to_string(),
        display: id.to_string(),
        search_text: id.to_string(),
        sort_key: Vec::new(),
        payload: PickerPayload::Theme,
    }
}

// endregion: --- Test Support

use agnt_runtime::protocol::persona::PersonaSummary;

use super::*;

/// Opening the agent picker queues the refresh hand the orchestrator answers.
#[test]
fn test_picker_agent_refresh_emits_on_open() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Agents);

    // -- Check: the refresh hand is waiting.
    assert!(state.take_next_agent_picker_refresh_request().is_some());
    Ok(())
}

/// Two opens before a single drain still produce exactly one agent refresh.
#[test]
fn test_picker_agent_refresh_dedups_before_drain() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Agents);
    open_shared_ui_action(&mut state, SharedUiAction::Agents);

    // -- Check: one hand, and the flag is cleared by the drain.
    assert!(state.take_next_agent_picker_refresh_request().is_some());
    assert!(state.take_next_agent_picker_refresh_request().is_none());
    Ok(())
}

/// Opening another picker does not queue an agent refresh.
#[test]
fn test_picker_agent_refresh_ignores_other_actions() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Models);

    // -- Check
    assert!(state.take_next_agent_picker_refresh_request().is_none());
    Ok(())
}

/// A fresh catalog replaces the picker rows and the Tab-cycle names together.
#[test]
fn test_picker_agent_refresh_sets_options_and_cycle_names() -> Result<()> {
    // -- Setup & Fixtures: a stale catalog that the refresh must replace.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_runtime::protocol::picker::AgentPickerOption {
            name: "stale".to_string(),
            description: None,
            display: "stale".to_string(),
            builtin: false,
        }],
    );
    state.status.identity.agent_cycle_names = vec!["stale".to_string()];
    open_shared_ui_action(&mut state, SharedUiAction::Agents);
    let reply = state
        .take_next_agent_picker_refresh_request()
        .ok_or("the agent picker queues its refresh hand")?;

    // -- Exec: the orchestrator answers the hand.
    reply.try_send(Ok(vec![
        PersonaSummary {
            name: "planner".to_string(),
            description: Some("Plans".to_string()),
            builtin: false,
        },
        PersonaSummary {
            name: "reviewer".to_string(),
            description: None,
            builtin: false,
        },
    ]))?;
    state.poll_picker_replies();

    // -- Check: the picker rows follow the scan, name-sorted.
    let ids: Vec<String> = state
        .picker
        .active_state()
        .ok_or("picker state should exist")?
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["planner", "reviewer"]);

    // -- Check: the Tab cycle names come from the same scan, in resolver order.
    assert_eq!(
        state.status.identity.agent_cycle_names,
        vec!["planner".to_string(), "reviewer".to_string()]
    );
    Ok(())
}

// region:    --- Test Support

/// Fold one shared UI action through the control fold and apply the launch it
/// queues, exactly as the live surface does.
fn open_shared_ui_action(state: &mut AppState, action: SharedUiAction) {
    fold_control(state, &ControlCommand::SharedUiAction { action });
    state.drain_launch_requests();
}

// endregion: --- Test Support

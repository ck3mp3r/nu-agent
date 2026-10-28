use agnt_core::protocol::persona::PersonaSummary;

use super::*;

/// Opening the agent picker queues a refresh request that drains as a
/// `RefreshAgentPicker` orchestrator event.
#[test]
fn test_picker_agent_refresh_emits_on_open() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check
    assert_eq!(agent_refresh_event_count(&events), 1);
    Ok(())
}

/// Two opens before a single drain still produce exactly one agent refresh.
#[test]
fn test_picker_agent_refresh_dedups_before_drain() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents));
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check: one request, and the flag is cleared by the drain.
    assert_eq!(agent_refresh_event_count(&events), 1);
    let drained_again = state.take_pending_events(&cancel_controller);
    assert_eq!(agent_refresh_event_count(&drained_again), 0);
    Ok(())
}

/// Opening another picker does not queue an agent refresh.
#[test]
fn test_picker_agent_refresh_ignores_other_actions() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check
    assert_eq!(agent_refresh_event_count(&events), 0);
    Ok(())
}

/// A fresh catalog replaces the picker rows and the Tab-cycle names together.
#[test]
fn test_picker_agent_refresh_sets_options_and_cycle_names() -> Result<()> {
    // -- Setup & Fixtures: a stale catalog that the refresh must replace.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: "stale".to_string(),
            description: None,
            display: "stale".to_string(),
            builtin: false,
        }],
    );
    state.status.identity.agent_cycle_names = vec!["stale".to_string()];
    state.picker.open(ActivePicker::Agent);

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::SetAgentPickerOptions(vec![
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
    ]));

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

fn agent_refresh_event_count(events: &[OrchestratorEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                OrchestratorEvent::UiRequest(UiRequest::RefreshAgentPicker)
            )
        })
        .count()
}

// endregion: --- Test Support

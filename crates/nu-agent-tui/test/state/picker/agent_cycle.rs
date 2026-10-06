use super::*;

#[test]
fn test_has_agents_to_cycle_empty() {
    let state = AppState::default();
    assert!(state.status.identity.agent_cycle_names.is_empty());
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_one() {
    let state = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_two() {
    let state = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    assert!(state.has_agents_to_cycle());
}

#[test]
fn test_next_agent_cycle_name_cycles() {
    let mut state = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };

    state.set_active_agent_identity("planner");
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));

    state.set_active_agent_identity("maker");
    assert_eq!(state.next_agent_cycle_name(), Some("planner".to_string()));
}

#[test]
fn test_next_agent_cycle_name_no_current() {
    let state = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));
}

#[test]
fn test_queue_cycle_agent_request() {
    let mut state = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    state.set_active_agent_identity("planner");

    state.queue_cycle_agent_request();

    let request = state.take_next_switch_request();
    assert_eq!(request, Some(SwitchRequest::Agent("maker".to_string())));
}

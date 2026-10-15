use super::*;

/// Agent cycling: `has_agents_to_cycle` needs two or more names, the next name
/// cycles from the active identity, and the cycle request queues a switch.
#[test]
fn agent_cycle_helpers() -> Result<()> {
    // -- Setup & Fixtures
    let empty = AppState::default();
    let one = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };
    let mut two = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };

    // -- Exec & Check: has_agents_to_cycle requires two or more names.
    assert!(empty.status.identity.agent_cycle_names.is_empty());
    assert!(!empty.has_agents_to_cycle());
    assert!(!one.has_agents_to_cycle());
    assert!(two.has_agents_to_cycle());

    // -- Exec & Check: the next name cycles from the active identity.
    two.set_active_agent_identity("planner");
    assert_eq!(two.next_agent_cycle_name(), Some("maker".to_string()));
    two.set_active_agent_identity("maker");
    assert_eq!(two.next_agent_cycle_name(), Some("planner".to_string()));

    // -- Setup & Fixtures: no current identity.
    let no_current = AppState {
        status: StatusState {
            identity: IdentityState {
                agent_cycle_names: vec!["planner".to_string(), "maker".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };

    // -- Exec & Check: with no current identity the next name is returned.
    assert_eq!(
        no_current.next_agent_cycle_name(),
        Some("maker".to_string())
    );

    // -- Exec & Check: the cycle request queues a switch to the next name.
    two.set_active_agent_identity("planner");
    two.queue_cycle_agent_request();
    assert_eq!(
        two.take_next_switch_request(),
        Some(SwitchRequest::Agent("maker".to_string()))
    );
    Ok(())
}

use super::*;

#[test]
fn command_palette_empty_query_returns_canonical_help_status_order_only() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert_eq!(
        palette_ids(&state),
        vec![
            "Help", "Status", "MCPs", "Skills", "Models", "Agents", "Sessions", "Theme",
        ]
    );
}

#[test]
fn command_palette_empty_query_returns_canonical_help_status_mcps_skills_order() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert_eq!(
        palette_ids(&state),
        vec![
            "Help", "Status", "MCPs", "Skills", "Models", "Agents", "Sessions", "Theme",
        ]
    );
}

#[test]
fn command_palette_fuzzy_matching_is_case_insensitive_and_non_prefix() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    for ch in "HP".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    assert_eq!(palette_ids(&state), vec!["Help"]);

    state.picker.active_state_mut().unwrap().query.clear();
    for ch in "tS".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    assert_eq!(palette_ids(&state), vec!["Status", "Agents"]);
}

#[test]
fn command_palette_fuzzy_query_matches_mcps_entry() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    for ch in "mcp".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }

    assert_eq!(palette_ids(&state), vec!["MCPs"]);
}

#[test]
fn command_palette_fuzzy_query_matches_skills_entry() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    for ch in "skls".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }

    assert_eq!(palette_ids(&state), vec!["Skills"]);
}

#[test]
fn command_palette_includes_models_action() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert!(palette_ids(&state).contains(&"Models".to_string()));
}

#[test]
fn command_palette_includes_agents_action() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert!(palette_ids(&state).contains(&"Agents".to_string()));
}

#[test]
fn info_panel_for_command_palette_action_agents_returns_none() {
    assert_eq!(CommandPaletteAction::Agents.info_panel(), None);
}

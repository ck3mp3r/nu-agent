use crate::state::*;
use nu_agent_core::protocol::slash::SlashCommand;

#[test]
fn command_palette_empty_query_returns_canonical_help_status_order_only() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert_eq!(
        state.command_palette_actions(),
        vec![
            CommandPaletteAction::Help,
            CommandPaletteAction::Status,
            CommandPaletteAction::Mcps,
            CommandPaletteAction::Skills,
            CommandPaletteAction::Models,
            CommandPaletteAction::Agents,
            CommandPaletteAction::Sessions,
            CommandPaletteAction::Theme,
        ]
    );
}

#[test]
fn command_palette_empty_query_returns_canonical_help_status_mcps_skills_order() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert_eq!(
        state.command_palette_actions(),
        vec![
            CommandPaletteAction::Help,
            CommandPaletteAction::Status,
            CommandPaletteAction::Mcps,
            CommandPaletteAction::Skills,
            CommandPaletteAction::Models,
            CommandPaletteAction::Agents,
            CommandPaletteAction::Sessions,
            CommandPaletteAction::Theme,
        ]
    );
}

#[test]
fn command_palette_fuzzy_matching_is_case_insensitive_and_non_prefix() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "HP".chars() {
        state.append_command_palette_query_char(ch);
    }
    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Help]
    );

    state.command_palette_query.clear();
    for ch in "tS".chars() {
        state.append_command_palette_query_char(ch);
    }
    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Status, CommandPaletteAction::Agents]
    );
}

#[test]
fn command_palette_fuzzy_query_matches_mcps_entry() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "mcp".chars() {
        state.append_command_palette_query_char(ch);
    }

    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Mcps]
    );
}

#[test]
fn command_palette_fuzzy_query_matches_skills_entry() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "skls".chars() {
        state.append_command_palette_query_char(ch);
    }

    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Skills]
    );
}

#[test]
fn command_palette_includes_models_action() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert!(
        state
            .command_palette_actions()
            .contains(&CommandPaletteAction::Models)
    );
}

#[test]
fn command_palette_includes_agents_action() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert!(
        state
            .command_palette_actions()
            .contains(&CommandPaletteAction::Agents)
    );
}

#[test]
fn info_panel_for_command_palette_action_agents_returns_none() {
    assert_eq!(CommandPaletteAction::Agents.info_panel(), None);
}

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::new();

    state.check_inline_slash("/");

    assert!(state.inline_slash_open);
    assert_eq!(state.inline_slash_selection, 0);
    assert_eq!(
        state.inline_slash_suggestions(),
        &[
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
            SlashCommand::Session,
            SlashCommand::Theme,
        ]
    );
}

#[test]
fn inline_slash_suggestions_filter_incrementally_as_input_grows() {
    let mut state = AppState::new();

    state.check_inline_slash("/");
    assert_eq!(state.inline_slash_suggestions().len(), 9);

    state.check_inline_slash("/c");
    assert_eq!(state.inline_slash_suggestions(), &[SlashCommand::Compact]);

    state.check_inline_slash("/co");
    assert_eq!(state.inline_slash_suggestions(), &[SlashCommand::Compact]);
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::new();

    state.check_inline_slash("/c");
    assert!(state.inline_slash_open);

    state.check_inline_slash("");

    assert!(!state.inline_slash_open);
    assert!(state.inline_slash_suggestions().is_empty());
}

#[test]
fn inline_slash_suggestions_do_not_open_command_palette() {
    let mut state = AppState::new();

    state.check_inline_slash("/");

    assert!(state.inline_slash_open);
    assert!(!state.command_palette_open);
}

#[test]
fn inline_model_picker_opens_with_available_models() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
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
    ]);

    state.open_model_picker();

    assert!(state.model_picker_open);
    assert_eq!(state.model_picker_selection, 0);
    assert_eq!(state.model_picker_query, "");
    assert_eq!(state.model_picker_filtered_options().len(), 2);
}

#[test]
fn inline_model_picker_filters_incrementally() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
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
    ]);
    state.open_model_picker();

    for ch in "openai".chars() {
        state.append_model_picker_query_char(ch);
    }

    let filtered = state.model_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].identity, "openai/gpt-4o-mini");
}

#[test]
fn inline_model_picker_moves_selection_deterministically() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
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
    ]);
    state.open_model_picker();

    state.model_picker_move_down();
    assert_eq!(state.model_picker_selection, 1);

    state.model_picker_move_down();
    assert_eq!(state.model_picker_selection, 0);

    state.model_picker_move_up();
    assert_eq!(state.model_picker_selection, 1);
}

#[test]
fn inline_model_picker_closes_on_escape() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        identity: "openai/gpt-4o-mini".to_string(),
        display: "openai / gpt-4o-mini".to_string(),
        active: true,
        context_window: None,
        max_output: None,
        configured: false,
        provider_display_name: String::new(),
    }]);
    state.open_model_picker();

    state.model_picker_close_on_escape();

    assert!(!state.model_picker_open);
    assert_eq!(state.model_picker_query, "");
    assert_eq!(state.model_picker_selection, 0);
}

#[test]
fn inline_model_picker_uses_cached_startup_plugin_config_catalog() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "z-provider".to_string(),
            model: "z-model".to_string(),
            identity: "z-provider/z-model".to_string(),
            display: "z-provider / z-model".to_string(),
            active: false,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
        },
        ModelPickerOption {
            provider: "a-provider".to_string(),
            model: "a-model".to_string(),
            identity: "a-provider/a-model".to_string(),
            display: "a-provider / a-model".to_string(),
            active: true,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
        },
    ]);
    state.open_model_picker();

    let ordered = state.model_picker_filtered_options();
    assert_eq!(ordered[0].identity, "a-provider/a-model");
    assert_eq!(ordered[1].identity, "z-provider/z-model");
}

#[test]
fn model_picker_query_changes_results_with_hydrated_catalog() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
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
    ]);
    state.open_model_picker();

    let all = state.model_picker_filtered_options();
    assert_eq!(all.len(), 2);

    for ch in "claude".chars() {
        state.append_model_picker_query_char(ch);
    }
    let narrowed = state.model_picker_filtered_options();
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].identity, "anthropic/claude-3-5-sonnet");
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::new();
    state.set_model_picker_options(Vec::new());
    state.open_model_picker();

    assert!(state.model_picker_open);
    assert!(state.model_picker_filtered_options().is_empty());
}

fn test_agent_options() -> Vec<AgentPickerOption> {
    vec![
        AgentPickerOption {
            name: "alpha".into(),
            description: Some("Alpha agent".into()),
            display: "alpha — Alpha agent".into(),
            active: false,
            builtin: false,
        },
        AgentPickerOption {
            name: "beta".into(),
            description: None,
            display: "beta".into(),
            active: true,
            builtin: false,
        },
        AgentPickerOption {
            name: "gamma".into(),
            description: Some("Gamma agent".into()),
            display: "gamma — Gamma agent".into(),
            active: false,
            builtin: false,
        },
    ]
}

#[test]
fn test_open_agent_picker() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.agent_picker_query = "leftover".to_string();
    state.agent_picker_selection = 2;

    state.open_agent_picker();

    assert!(state.agent_picker_open);
    assert_eq!(state.agent_picker_query, "");
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn test_close_agent_picker() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();
    state.agent_picker_query = "al".to_string();
    state.agent_picker_selection = 1;

    state.close_agent_picker();

    assert!(!state.agent_picker_open);
    assert_eq!(state.agent_picker_query, "");
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn test_queue_agent_picker_launch_request() {
    let mut state = AppState::new();

    state.queue_agent_picker_launch_request();

    assert!(state.take_next_agent_picker_launch_request());
}

#[test]
fn test_take_next_agent_picker_launch_request() {
    let mut state = AppState::new();

    // No pending requests
    assert!(!state.take_next_agent_picker_launch_request());

    // Queue one
    state.queue_agent_picker_launch_request();
    assert!(state.take_next_agent_picker_launch_request());

    // Consumed — should return false again
    assert!(!state.take_next_agent_picker_launch_request());
}

#[test]
fn test_filtered_agent_picker_options_empty_query() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_filtered_agent_picker_options_with_query() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Case-insensitive match on name
    for ch in "ALPHA".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "alpha");

    // Reset and test display match
    state.agent_picker_query.clear();
    for ch in "Gamma agent".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "gamma");
}

#[test]
fn test_filtered_agent_picker_options_no_match() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    for ch in "zzz".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert!(filtered.is_empty());
}

#[test]
fn test_selected_agent_picker_option() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // set_agent_picker_options sorts by name: alpha, beta, gamma
    let first = state.selected_agent_picker_option();
    assert_eq!(first.unwrap().name, "alpha");

    state.agent_picker_move_down();
    let second = state.selected_agent_picker_option();
    assert_eq!(second.unwrap().name, "beta");

    state.agent_picker_move_down();
    let third = state.selected_agent_picker_option();
    assert_eq!(third.unwrap().name, "gamma");
}

#[test]
fn test_queue_selected_agent_switch_request() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Select second item (beta)
    state.agent_picker_move_down();
    assert!(state.queue_selected_agent_switch_request());

    let request = state.take_next_agent_switch_request();
    assert_eq!(request, Some("beta".to_string()));
}

#[test]
fn test_take_next_agent_switch_request() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Queue two requests in order: alpha then beta
    assert!(state.queue_selected_agent_switch_request()); // alpha (index 0)
    state.agent_picker_move_down();
    assert!(state.queue_selected_agent_switch_request()); // beta (index 1)

    // FIFO order
    assert_eq!(
        state.take_next_agent_switch_request(),
        Some("alpha".to_string())
    );
    assert_eq!(
        state.take_next_agent_switch_request(),
        Some("beta".to_string())
    );
    assert_eq!(state.take_next_agent_switch_request(), None);
}

#[test]
fn test_set_active_agent_identity() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());

    state.set_active_agent_identity("beta");

    // Only "beta" should be active
    let options = &state.agent_picker_options;
    for opt in options {
        if opt.name == "beta" {
            assert!(opt.active, "beta should be active");
        } else {
            assert!(!opt.active, "{} should not be active", opt.name);
        }
    }
    assert_eq!(state.active_agent_identity(), Some("beta"));
}

#[test]
fn test_agent_picker_move_up_wraps() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    assert_eq!(state.agent_picker_selection, 0);

    // Move up from 0 should wrap to last (index 2)
    state.agent_picker_move_up();
    assert_eq!(state.agent_picker_selection, 2);
}

#[test]
fn test_agent_picker_move_down_wraps() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Move to last
    state.agent_picker_move_down();
    state.agent_picker_move_down();
    assert_eq!(state.agent_picker_selection, 2);

    // Move down from last should wrap to 0
    state.agent_picker_move_down();
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn test_has_agents_to_cycle_empty() {
    let state = AppState::new();
    assert!(state.agent_cycle_names.is_empty());
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_one() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string()];
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_two() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    assert!(state.has_agents_to_cycle());
}

#[test]
fn test_next_agent_cycle_name_cycles() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];

    // Set active to "planner" → next should be "maker"
    state.set_active_agent_identity("planner");
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));

    // Set active to "maker" → next should wrap to "planner"
    state.set_active_agent_identity("maker");
    assert_eq!(state.next_agent_cycle_name(), Some("planner".to_string()));
}

#[test]
fn test_next_agent_cycle_name_no_current() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    // active_agent_identity is None → unwrap_or("") → position not found → unwrap_or(0)
    // next_idx = (0 + 1) % 2 = 1 → "maker"
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));
}

#[test]
fn test_queue_cycle_agent_request() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");

    state.queue_cycle_agent_request();

    let request = state.take_next_agent_switch_request();
    assert_eq!(request, Some("maker".to_string()));
}

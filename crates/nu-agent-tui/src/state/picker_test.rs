use crate::interaction::dispatch::dispatch_terminal_event;
use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::state::*;
use crate::test_support::open_command_palette_for_test;
use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn palette_ids(state: &AppState) -> Vec<String> {
    state
        .picker
        .active_state()
        .unwrap()
        .filtered()
        .iter()
        .map(|o| o.id.clone())
        .collect()
}

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

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::default();

    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.selection, 0);
    let ids: Vec<String> = s.options.iter().map(|o| o.id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            "/compact", "/mcp", "/help", "/status", "/models", "/agent", "/new", "/session",
            "/theme", "/skills",
        ]
    );
}

#[test]
fn inline_slash_suggestions_filter_incrementally_as_input_grows() {
    let mut state = AppState::default();

    state.check_inline_slash("/");
    assert_eq!(state.picker.active_state().unwrap().options.len(), 10);

    state.check_inline_slash("/c");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);

    state.check_inline_slash("/co");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::default();

    state.check_inline_slash("/c");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

    state.check_inline_slash("");

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn inline_slash_suggestions_do_not_open_command_palette() {
    let mut state = AppState::default();

    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

fn test_model_options() -> Vec<ModelPickerOption> {
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

#[test]
fn inline_model_picker_opens_with_available_models() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.selection, 0);
    assert_eq!(s.query, "");
    assert_eq!(s.filtered().len(), 2);
}

#[test]
fn inline_model_picker_filters_incrementally() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    for ch in "openai".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }

    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "openai/gpt-4o-mini");
}

#[test]
fn inline_model_picker_moves_selection_deterministically() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    state.picker.active_state_mut().unwrap().move_up();
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn inline_model_picker_closes_on_escape() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    state.picker.close();

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn inline_model_picker_uses_cached_startup_plugin_config_catalog() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
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
        ],
    );
    state.picker.open(ActivePicker::Model);

    let ordered = state.picker.active_state().unwrap().filtered();
    assert_eq!(ordered[0].id, "a-provider/a-model");
    assert_eq!(ordered[1].id, "z-provider/z-model");
}

#[test]
fn model_picker_query_changes_results_with_hydrated_catalog() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    let all = state.picker.active_state().unwrap().filtered();
    assert_eq!(all.len(), 2);

    for ch in "claude".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let narrowed = state.picker.active_state().unwrap().filtered();
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].id, "anthropic/claude-3-5-sonnet");
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert!(state.picker.active_state().unwrap().filtered().is_empty());
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
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "leftover".to_string();
        s.selection = 2;
    }

    state.picker.open(ActivePicker::Agent);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.query, "");
    assert_eq!(s.selection, 0);
}

#[test]
fn test_close_agent_picker() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "al".to_string();
        s.selection = 1;
    }

    state.picker.close();

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn test_queue_agent_picker_launch_request() {
    let mut state = AppState::default();

    state.queue_launch_request(SharedUiAction::Agents);

    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
}

#[test]
fn test_take_next_agent_picker_launch_request() {
    let mut state = AppState::default();

    assert_eq!(state.take_next_launch_request(), None);

    state.queue_launch_request(SharedUiAction::Agents);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );

    assert_eq!(state.take_next_launch_request(), None);
}

#[test]
fn test_filtered_agent_picker_options_empty_query() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_filtered_agent_picker_options_with_query() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    for ch in "ALPHA".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "alpha");

    state.picker.active_state_mut().unwrap().query.clear();
    for ch in "Gamma agent".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "gamma");
}

#[test]
fn test_filtered_agent_picker_options_no_match() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    for ch in "zzz".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert!(filtered.is_empty());
}

#[test]
fn test_selected_agent_picker_option() -> Result<()> {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let first = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have first selected option")?;
    assert_eq!(first.id, "alpha");

    state.picker.active_state_mut().unwrap().move_down();
    let second = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have second selected option")?;
    assert_eq!(second.id, "beta");

    state.picker.active_state_mut().unwrap().move_down();
    let third = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have third selected option")?;
    assert_eq!(third.id, "gamma");
    Ok(())
}

#[test]
fn test_queue_selected_agent_switch_request() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    state.picker.active_state_mut().unwrap().move_down();
    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));

    let request = state.take_next_switch_request();
    assert_eq!(request, Some(SwitchRequest::Agent("beta".to_string())));
}

#[test]
fn test_take_next_agent_switch_request() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));
    state.picker.active_state_mut().unwrap().move_down();
    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));

    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("alpha".to_string()))
    );
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );
    assert_eq!(state.take_next_switch_request(), None);
}

#[test]
fn test_set_active_agent_identity() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());

    state.set_active_agent_identity("beta");

    let options = &state.picker.entries[2].state.options;
    for opt in options {
        let name = match &opt.payload {
            PickerPayload::Agent { name, .. } => name.clone(),
            _ => String::new(),
        };
        let active = match &opt.payload {
            PickerPayload::Agent { active, .. } => *active,
            _ => false,
        };
        if name == "beta" {
            assert!(active, "beta should be active");
        } else {
            assert!(!active, "{} should not be active", name);
        }
    }
    assert_eq!(state.status.identity.active_agent_identity(), Some("beta"));
}

#[test]
fn test_agent_picker_move_up_wraps() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    state.picker.active_state_mut().unwrap().move_up();
    assert_eq!(state.picker.active_state().unwrap().selection, 2);
}

#[test]
fn test_agent_picker_move_down_wraps() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    state.picker.active_state_mut().unwrap().move_down();
    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 2);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
}

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

#[test]
fn picker_state_new_has_defaults() {
    let state = PickerState::<PickerOption>::default();
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
    assert!(state.options.is_empty());
}

#[test]
fn picker_state_open_resets_query_and_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    assert!(state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_close_resets_query_and_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.close();
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_move_up_wraps() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.move_up();
    assert_eq!(state.selection, 2);
    state.move_up();
    assert_eq!(state.selection, 1);
}

#[test]
fn picker_state_move_down_wraps() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.move_down();
    assert_eq!(state.selection, 1);
    state.move_down();
    assert_eq!(state.selection, 2);
    state.move_down();
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_move_up_empty_resets_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.move_up();
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_append_query_char_resets_selection() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 2,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.append_query_char('a');
    assert_eq!(state.query, "a");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_backspace_query_char() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.append_query_char('a');
    state.append_query_char('b');
    state.backspace_query_char();
    assert_eq!(state.query, "a");
}

#[test]
fn picker_state_clamp_selection() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 5,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.clamp_selection(3);
    assert_eq!(state.selection, 2);
    state.selection = 0;
    state.clamp_selection(0);
    assert_eq!(state.selection, 0);
}

fn picker_option(id: &str) -> PickerOption {
    PickerOption {
        id: id.to_string(),
        display: id.to_string(),
        search_text: id.to_string(),
        payload: PickerPayload::Theme,
    }
}

#[test]
fn picker_container_active_none_by_default() {
    let container = PickerContainer::default();
    assert_eq!(container.active(), None);
}

#[test]
fn picker_container_open_command_palette_sets_active() {
    let mut container = PickerContainer::default();
    container.open(ActivePicker::CommandPalette);
    assert_eq!(container.active(), Some(ActivePicker::CommandPalette));
}

#[test]
fn picker_container_close_command_palette_clears_active() {
    let mut container = PickerContainer::default();
    container.open(ActivePicker::CommandPalette);
    container.close();
    assert_eq!(container.active(), None);
}

#[test]
fn open_model_picker_closes_command_palette() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_eq!(state.picker.active(), Some(ActivePicker::Model));
}

#[test]
fn open_command_palette_closes_inline_slash() {
    let mut state = AppState::default();
    state.check_inline_slash("/");
    open_command_palette_for_test(&mut state);

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active(), Some(ActivePicker::CommandPalette));
}

#[test]
fn set_picker_options_sorts_agent_by_id() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            AgentPickerOption {
                name: "zeta".into(),
                description: None,
                display: "zeta".into(),
                active: false,
                builtin: false,
            },
            AgentPickerOption {
                name: "alpha".into(),
                description: None,
                display: "alpha".into(),
                active: false,
                builtin: false,
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[2]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["alpha", "zeta"]);
}

#[test]
fn set_picker_options_sorts_session_by_created_at_desc() {
    let mut state = AppState::default();
    let now = chrono::Utc::now();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            PickerOption {
                id: "old".to_string(),
                display: "old".to_string(),
                search_text: "old".to_string(),
                payload: PickerPayload::Session {
                    session_id: "old".to_string(),
                    title: None,
                    created_at: now - chrono::Duration::days(1),
                },
            },
            PickerOption {
                id: "new".to_string(),
                display: "new".to_string(),
                search_text: "new".to_string(),
                payload: PickerPayload::Session {
                    session_id: "new".to_string(),
                    title: None,
                    created_at: now,
                },
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[3]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["new", "old"]);
}

#[test]
fn set_picker_options_keeps_theme_unsorted() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Theme,
        vec![
            PickerOption {
                id: "b".to_string(),
                display: "b".to_string(),
                search_text: "b".to_string(),
                payload: PickerPayload::Theme,
            },
            PickerOption {
                id: "a".to_string(),
                display: "a".to_string(),
                search_text: "a".to_string(),
                payload: PickerPayload::Theme,
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[4]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["b", "a"]);
}

#[test]
fn set_picker_options_writes_without_opening() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());

    assert_eq!(state.picker.render_kind(), None);
    assert!(state.picker.entries[1].state.options.is_empty());
}

#[test]
fn set_picker_options_sorts_model_by_provider_then_identity() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            ModelPickerOption {
                provider: "z-provider".to_string(),
                model: "z-model".to_string(),
                identity: "z-provider/z-model".to_string(),
                display: "z".to_string(),
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
                display: "a".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[1]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["a-provider/a-model", "z-provider/z-model"]);
}

#[test]
fn open_command_palette_twice_opens_single_picker() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    open_command_palette_for_test(&mut state);

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active(), Some(ActivePicker::CommandPalette));
}

fn test_session_options() -> Vec<PickerOption> {
    let now = chrono::Utc::now();
    vec![
        PickerOption {
            id: "old".to_string(),
            display: "old".to_string(),
            search_text: "old".to_string(),
            payload: PickerPayload::Session {
                session_id: "old".to_string(),
                title: None,
                created_at: now - chrono::Duration::days(1),
            },
        },
        PickerOption {
            id: "new".to_string(),
            display: "new".to_string(),
            search_text: "new".to_string(),
            payload: PickerPayload::Session {
                session_id: "new".to_string(),
                title: None,
                created_at: now,
            },
        },
    ]
}

fn type_query(state: &mut AppState, query: &str) -> Result<()> {
    for ch in query.chars() {
        state
            .picker
            .active_state_mut()
            .ok_or("picker should be open")?
            .append_query_char(ch);
    }
    Ok(())
}

#[test]
fn picker_submit_without_selection_queues_no_switch_request() -> Result<()> {
    // -- Exec & Check
    for kind in [
        ActivePicker::Model,
        ActivePicker::Agent,
        ActivePicker::Session,
    ] {
        // Empty catalog: picker opened before options arrive.
        let mut state = AppState::default();
        state.set_picker_options(kind, Vec::<PickerOption>::new());
        state.picker.open(kind);

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

        assert!(changed, "{kind:?} submit should be consumed");
        assert_eq!(state.picker.render_kind(), None, "{kind:?} should close");
        assert_eq!(
            state.take_next_switch_request(),
            None,
            "{kind:?} must not queue a switch request"
        );

        // Hydrated catalog with a query that matches zero options.
        let mut state = AppState::default();
        match kind {
            ActivePicker::Model => state.set_picker_options(kind, test_model_options()),
            ActivePicker::Agent => state.set_picker_options(kind, test_agent_options()),
            _ => state.set_picker_options(kind, test_session_options()),
        }
        state.picker.open(kind);
        type_query(&mut state, "zzz")?;

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

        assert!(changed, "{kind:?} submit should be consumed");
        assert_eq!(state.picker.render_kind(), None, "{kind:?} should close");
        assert_eq!(
            state.take_next_switch_request(),
            None,
            "{kind:?} must not queue a switch request"
        );
    }
    Ok(())
}

#[test]
fn command_palette_submit_with_zero_matches_opens_no_panel() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    type_query(&mut state, "zzz")?;

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check
    assert!(changed, "Enter should be consumed by the palette");
    assert_eq!(state.picker.render_kind(), None, "palette should close");
    assert_eq!(state.info_panel, None, "Help panel must not open");
    assert_eq!(
        state.take_next_launch_request(),
        None,
        "no launch request may be queued"
    );
    Ok(())
}

#[test]
fn picker_submit_with_selection_resolves_payload_switch_action() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check
    assert!(changed, "submit should be handled");
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Model(
            "anthropic/claude-3-5-sonnet".to_string()
        )),
        "selection must resolve to its payload identity, not the placeholder"
    );
    assert_eq!(
        state.picker.render_kind(),
        None,
        "picker closes after submit"
    );
    Ok(())
}

use super::*;
use agnt_core::protocol::slash::SlashCommand;

/// Every picker payload kind filters through the one shared subsequence
/// matcher: a query whose characters are not contiguous in the row still
/// matches, decoys without those characters drop out, and an empty query keeps
/// every row.
#[test]
fn picker_fuzzy_matching_covers_every_payload_kind() -> Result<()> {
    // -- Setup & Fixtures: a matching row and a decoy per payload kind.
    let cases: Vec<(&str, Vec<PickerOption>, &str, Vec<&str>)> = vec![
        (
            "command",
            vec![
                command_row(CommandPaletteAction::Help, "Help", "help info panel"),
                command_row(CommandPaletteAction::Status, "Status", "status panel"),
            ],
            "hp",
            vec!["Help"],
        ),
        (
            "slash",
            vec![
                slash_row(SlashCommand::Compact),
                slash_row(SlashCommand::Mcp),
            ],
            "/cpt",
            vec!["/compact"],
        ),
        (
            "model",
            vec![
                model_row("local/qwen3.8", "local / Qwen3.8"),
                model_row("openai/gpt-4o-mini", "openai / gpt-4o-mini"),
            ],
            "q38",
            vec!["local/qwen3.8"],
        ),
        (
            "agent",
            vec![
                agent_row("alpha", Some("Alpha agent")),
                agent_row("beta", None),
            ],
            "bt",
            vec!["beta"],
        ),
        (
            "session",
            vec![
                session_row("s1", "old work"),
                session_row("s2", "new notes"),
            ],
            "nw",
            vec!["s2"],
        ),
        (
            "theme",
            vec![
                theme_row("CatppuccinMocha", "Catppuccin Mocha"),
                theme_row("Dracula", "Dracula"),
            ],
            "cpm",
            vec!["CatppuccinMocha"],
        ),
    ];

    for (kind, rows, query, expected) in cases {
        // -- Exec & Check: an empty query keeps every row.
        let all = filtered_ids(&rows, "")?;
        assert_eq!(all.len(), rows.len(), "{kind}: empty query keeps all rows");

        // -- Exec & Check: the non-contiguous query narrows to the intended row.
        let matched = filtered_ids(&rows, query)?;
        assert_eq!(matched, expected, "{kind}: query {query}");

        // -- Exec & Check: a query with no subsequence drops every row.
        let none = filtered_ids(&rows, "zzz")?;
        assert!(none.is_empty(), "{kind}: zzz should match nothing");
    }
    Ok(())
}

/// The command palette keeps subsequence matching after the unification: the
/// real palette fixture still resolves a non-contiguous query.
#[test]
fn command_palette_keeps_fuzzy_matching() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec & Check: a non-contiguous, case-insensitive query.
    type_query(&mut state, "HP")?;
    assert_eq!(palette_ids(&state)?, vec!["Help"]);

    // -- Exec & Check: a non-contiguous query across the id.
    state
        .picker
        .active_state_mut()
        .ok_or("palette should be open")?
        .query
        .clear();
    type_query(&mut state, "skls")?;
    assert_eq!(palette_ids(&state)?, vec!["Skills"]);
    Ok(())
}

// region:    --- Test Support

fn filtered_ids(rows: &[PickerOption], query: &str) -> Result<Vec<String>> {
    let state = PickerState::<PickerOption> {
        open: false,
        query: query.to_string(),
        selection: 0,
        options: rows.to_vec(),
    };
    Ok(state.filtered().iter().map(|o| o.id.clone()).collect())
}

fn command_row(action: CommandPaletteAction, display: &str, search_text: &str) -> PickerOption {
    PickerOption {
        id: display.to_string(),
        display: display.to_string(),
        search_text: search_text.to_string(),
        section: None,
        sort_key: Vec::new(),
        payload: PickerPayload::Command(action),
    }
}

fn slash_row(command: SlashCommand) -> PickerOption {
    let label = command.label().to_string();
    PickerOption {
        id: label.clone(),
        display: label.clone(),
        search_text: label,
        section: None,
        sort_key: Vec::new(),
        payload: PickerPayload::Slash(command),
    }
}

fn model_row(identity: &str, display: &str) -> PickerOption {
    PickerOption::from(ModelPickerOption {
        provider: "local".to_string(),
        model: identity.to_string(),
        identity: identity.to_string(),
        display: display.to_string(),
        active: false,
        context_window: None,
        max_output: None,
        configured: false,
        provider_display_name: String::new(),
        cost: None,
    })
}

fn agent_row(name: &str, description: Option<&str>) -> PickerOption {
    PickerOption::from(AgentPickerOption {
        name: name.to_string(),
        description: description.map(str::to_string),
        display: name.to_string(),
        builtin: false,
    })
}

fn session_row(id: &str, title: &str) -> PickerOption {
    let created_at = chrono::Utc::now();
    PickerOption {
        id: id.to_string(),
        display: title.to_string(),
        search_text: title.to_string(),
        section: Some(crate::state::session_day(created_at).to_string()),
        sort_key: Vec::new(),
        payload: PickerPayload::Session {
            session_id: id.to_string(),
            title: Some(title.to_string()),
            created_at,
            message_count: 1,
        },
    }
}

fn theme_row(id: &str, display: &str) -> PickerOption {
    PickerOption {
        id: id.to_string(),
        display: display.to_string(),
        search_text: display.to_string(),
        section: None,
        sort_key: Vec::new(),
        payload: PickerPayload::Theme,
    }
}

// endregion: --- Test Support

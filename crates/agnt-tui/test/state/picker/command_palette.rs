use super::*;

/// The command palette lists the canonical actions in order, filters
/// case-insensitively on non-prefix queries, and exposes no info panel for the
/// Agents action.
#[test]
fn command_palette_order_and_fuzzy_matching() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec & Check: the canonical order.
    let canonical = vec![
        "Help", "Status", "MCPs", "Skills", "Models", "Agents", "Sessions", "Theme",
    ];
    assert_eq!(palette_ids(&state)?, canonical);
    assert!(palette_ids(&state)?.contains(&"Models".to_string()));
    assert!(palette_ids(&state)?.contains(&"Agents".to_string()));

    // -- Exec & Check: a case-insensitive non-prefix query.
    type_query(&mut state, "HP")?;
    assert_eq!(palette_ids(&state)?, vec!["Help"]);

    // -- Exec & Check: a query matching multiple entries.
    state
        .picker
        .active_state_mut()
        .ok_or("palette should be open")?
        .query
        .clear();
    type_query(&mut state, "tS")?;
    assert_eq!(palette_ids(&state)?, vec!["Status", "Agents"]);

    // -- Exec & Check: fuzzy queries match MCPs and Skills.
    state
        .picker
        .active_state_mut()
        .ok_or("palette should be open")?
        .query
        .clear();
    type_query(&mut state, "mcp")?;
    assert_eq!(palette_ids(&state)?, vec!["MCPs"]);

    state
        .picker
        .active_state_mut()
        .ok_or("palette should be open")?
        .query
        .clear();
    type_query(&mut state, "skls")?;
    assert_eq!(palette_ids(&state)?, vec!["Skills"]);

    // -- Exec & Check: the Agents action has no info panel.
    assert_eq!(CommandPaletteAction::Agents.info_panel(), None);
    Ok(())
}

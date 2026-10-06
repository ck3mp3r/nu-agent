use super::*;

#[test]
fn help_panel_renders_required_sections_in_contract_order() {
    let (title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    assert_eq!(title, "Help");
    let rendered_lines = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let required_sections = [
        "Getting started",
        "Modes (insert vs normal)",
        "Core keys (with explanations)",
        "Command palette (Ctrl-P)",
        "MCP basics (enabled/disabled/failed + where to toggle)",
        "Troubleshooting",
    ];

    let mut previous_index = None;
    for section in required_sections {
        let section_index = rendered_lines
            .iter()
            .position(|line| line.trim() == section)
            .unwrap_or_else(|| panic!("missing section heading: {section}"));
        if let Some(previous_index) = previous_index {
            assert!(
                section_index > previous_index,
                "section out of order: {section}"
            );
        }
        previous_index = Some(section_index);
    }
}

#[test]
fn help_panel_copy_is_plain_language_and_includes_ctrl_p_and_mcp_basics() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("Ctrl-P"),
        "help should explicitly mention Ctrl-P"
    );
    assert!(
        rendered.contains("open the command palette"),
        "Ctrl-P entry should explain intent in plain language"
    );
    assert!(
        rendered.contains("enabled")
            && rendered.contains("disabled")
            && rendered.contains("failed")
            && rendered.contains("command palette")
            && rendered.contains("MCPs"),
        "MCP basics should describe state meanings and where to toggle"
    );

    assert!(
        rendered.contains("server is") || rendered.contains("return to normal mode"),
        "help copy should include explanatory language rather than key-only jargon"
    );
}

#[test]
fn help_panel_markdown_projection_preserves_supported_formatting() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("• "),
        "markdown bullet lists should be projected as list markers"
    );
    let inline_code_spans = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.content.contains("Ctrl-P") || span.content.contains("Esc"))
        .count();
    assert!(
        inline_code_spans > 0,
        "inline code content should survive markdown projection"
    );
}

#[test]
fn help_panel_scroll_can_reach_final_content_line_with_keyboard_scroll_model() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let viewport_inner_height = 8u16;
    let max_scroll = help_panel_max_scroll_for_test(&lines, viewport_inner_height);
    let window = help_panel_visible_window_for_test(&lines, viewport_inner_height, max_scroll);
    let rendered = window
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("If text looks stale after resize"),
        "scroll window should reach help footer content"
    );
}

#[test]
fn help_panel_shows_overflow_position_cue_when_content_exceeds_viewport() -> Result<()> {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let viewport_inner_height = 8u16;
    let cue = help_panel_overflow_cue_for_test(&lines, viewport_inner_height, 3)
        .ok_or("should have overflow cue when help exceeds viewport")?;

    assert!(cue.contains("PgUp/PgDn"));
    assert!(cue.contains("Esc close"));
    assert!(cue.contains("/"));
    Ok(())
}

#[tokio::test]
async fn help_panel_escape_closes_panel_after_scroll() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Help);
    // AppState.info_panel_scroll is a public field — no setter method exists.
    driver.coordinator_mut().state.info_panel_scroll = 5;

    // -- Exec & Check: Down scrolls (or holds) the panel through the real arm.
    driver.advance(&[key(TerminalKey::Down)]).await?;
    assert!(driver.state().info_panel_scroll >= 5);

    // -- Exec & Check: Esc closes the panel through the real arm.
    driver.advance(&[key(TerminalKey::Esc)]).await?;
    assert_eq!(driver.state().info_panel, None);
    Ok(())
}

#[test]
fn skills_panel_renders_empty_state_when_no_skills_available() {
    let state = AppState::default();
    let (title, lines) = crate::runtime::skills_panel_lines(&state);

    assert_eq!(title, "Skills");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("No discoverable skills available."));
}

#[test]
fn skills_panel_lists_skills_in_deterministic_order() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_discoverable_skills(vec![
        crate::state::DiscoverableSkill {
            source_priority: 1,
            source: "home".to_string(),
            name: "zeta".to_string(),
            description: Some("Zeta skills".to_string()),
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "beta".to_string(),
            description: None,
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "alpha".to_string(),
            description: Some("Alpha agent".to_string()),
        },
    ]);

    let (_title, lines) = crate::runtime::skills_panel_lines(&state);
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let alpha_idx = rendered.find("alpha").ok_or("should find alpha row")?;
    let beta_idx = rendered.find("beta").ok_or("should find beta row")?;
    let zeta_idx = rendered.find("zeta").ok_or("should find zeta row")?;
    assert!(alpha_idx < beta_idx);
    assert!(beta_idx < zeta_idx);
    assert!(
        rendered.contains("- alpha (repo) — Alpha agent"),
        "skill with description must render the trailing description; got:\n{rendered}"
    );
    assert!(
        rendered.contains("- beta (repo)\n"),
        "skill with None description must render no separator; got:\n{rendered}"
    );
    Ok(())
}

#[test]
fn help_panel_scroll_offset_applied() {
    // A viewport smaller than the help content forces a nonzero max scroll.
    // Requesting a large offset must be clamped to the max; requesting a small
    // offset must be returned unchanged.
    let viewport_height = 8u16;
    let viewport_width = 80u16;

    let small_scroll = super::help_panel_scroll_offset_for_test(viewport_height, viewport_width, 3);
    assert_eq!(
        small_scroll, 3,
        "scroll offset below max must pass through unchanged"
    );

    let huge_scroll =
        super::help_panel_scroll_offset_for_test(viewport_height, viewport_width, usize::MAX);
    let max_scroll = super::help_panel_max_scroll_for_test(
        &super::help_panel_lines(&crate::rendering::theme::TuiTheme::default()).1,
        viewport_height,
    );
    assert_eq!(
        huge_scroll, max_scroll,
        "scroll offset above max must be clamped to max"
    );
    assert!(max_scroll > 0, "help content must exceed small viewport");
}

#[test]
fn status_panel_scroll_offset_applied() {
    // Status content is short; with a large viewport the max scroll is 0 and
    // any requested offset is clamped to 0. With a very small viewport the
    // offset should be clamped correctly.
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: crate::state::McpServerUsabilityState::Enabled,
        }]);

    let viewport_height = 3u16; // smaller than status content
    let viewport_width = 80u16;

    let small_scroll =
        super::status_panel_scroll_offset_for_test(&state, viewport_height, viewport_width, 1);
    // 1 is within content so it should pass through (or be clamped if content
    // is ≤ 3 lines — either outcome is correct as long as it's ≤ max).
    let (_title, lines) = super::status_panel_lines(&state);
    let max = super::help_panel_max_scroll_for_test(&lines, viewport_height);
    assert!(
        small_scroll <= max,
        "scroll offset must not exceed max (got {small_scroll}, max {max})"
    );

    let huge_scroll = super::status_panel_scroll_offset_for_test(
        &state,
        viewport_height,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(
        huge_scroll, max,
        "scroll offset above max must be clamped to max"
    );
}

#[test]
fn skills_panel_scroll_offset_applied() {
    let mut state = AppState::default();
    state.status.mcp.set_discoverable_skills(vec![
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "alpha".to_string(),
            description: None,
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "beta".to_string(),
            description: None,
        },
    ]);

    let viewport_height = 2u16; // smaller than content (header + 2 skill lines)
    let viewport_width = 80u16;

    let (_title, lines) = super::skills_panel_lines(&state);
    let max = super::help_panel_max_scroll_for_test(&lines, viewport_height);

    let huge_scroll = super::skills_panel_scroll_offset_for_test(
        &state,
        viewport_height,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(
        huge_scroll, max,
        "scroll offset above max must be clamped to max"
    );

    let zero_scroll =
        super::skills_panel_scroll_offset_for_test(&state, viewport_height, viewport_width, 0);
    assert_eq!(zero_scroll, 0, "zero scroll must pass through as zero");
}

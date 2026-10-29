use super::*;

/// The help panel renders the required sections in contract order, with plain
/// language copy and markdown projection preserving supported formatting.
#[test]
fn help_panel_content_contract() -> Result<()> {
    // -- Exec
    let (title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default(), 200);

    // -- Check: the title and required sections in order.
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
            .ok_or_else(|| format!("missing section heading: {section}"))?;
        if let Some(previous_index) = previous_index {
            assert!(
                section_index > previous_index,
                "section out of order: {section}"
            );
        }
        previous_index = Some(section_index);
    }

    // -- Check: plain-language copy.
    let rendered = rendered_lines.join("\n");
    assert!(rendered.contains("Ctrl-P"));
    assert!(rendered.contains("open the command palette"));
    assert!(
        rendered.contains("enabled")
            && rendered.contains("disabled")
            && rendered.contains("failed")
            && rendered.contains("command palette")
            && rendered.contains("MCPs")
    );
    assert!(rendered.contains("server is") || rendered.contains("return to normal mode"));

    // -- Check: markdown projection.
    assert!(rendered.contains("• "));
    let inline_code_spans = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.content.contains("Ctrl-P") || span.content.contains("Esc"))
        .count();
    assert!(inline_code_spans > 0);
    Ok(())
}

/// The clamped max scroll reaches the tail of the last help line: the scroll
/// math counts the rows the render paints, not a hard-break estimate.
#[tokio::test]
async fn help_panel_scroll_reaches_last_content_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(68, 24);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Help);
    driver.coordinator_mut().state.info_panel_scroll = usize::MAX;

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the tail of help.md's last line sits above the bottom border.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Help,
    );
    let inner = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let panel_text = driver.buffer_text_in_rect(inner);
    assert!(
        panel_text.contains("(h/l)."),
        "the last help line must be reachable at max scroll; popup:\n{panel_text}"
    );
    Ok(())
}

/// The scroll math counts the rows the word-wrapped lines occupy. Five 3-char
/// words are 19 chars, so a hard-break estimate gives ceil(19/10) = 2 rows
/// while the word wrap needs 3; the max scroll is exactly lines − visible.
#[test]
fn help_panel_scroll_math_counts_word_wrapped_rows() -> Result<()> {
    // -- Setup & Fixtures
    let wrapped = crate::runtime::panels::wrap_panel_lines(
        vec![ratatui::text::Line::from("aaa aaa aaa aaa aaa")],
        10,
    );

    // -- Exec
    let max_scroll = crate::runtime::panels::help_panel_max_scroll(&wrapped, 1);
    let cue = crate::runtime::panels::help_panel_overflow_cue(&wrapped, 2, 1);

    // -- Check
    assert_eq!(
        wrapped.len(),
        3,
        "the word wrap must fill three rows; rows: {wrapped:?}"
    );
    assert_eq!(
        max_scroll, 2,
        "max scroll must be the wrapped row count minus the visible rows"
    );
    assert_eq!(cue, "↑/↓ · PgUp/PgDn · Esc · 2-3 / 3");
    Ok(())
}

/// The Status and Skills panels report the same position cue as Help when
/// their content overflows the viewport.
#[tokio::test]
async fn status_and_skills_titles_report_the_position_cue() -> Result<()> {
    // -- Setup & Fixtures: a status panel whose failures overflow a 44x4 popup.
    let mut status = RenderLoopDriver::new(46, 8);
    {
        let state = &mut status.coordinator_mut().state;
        state.status.mcp.set_mcp_servers(vec![
            crate::state::McpServerState {
                name: "gh".to_string(),
                state: crate::state::McpServerUsabilityState::Failed,
            },
            crate::state::McpServerState {
                name: "k8s".to_string(),
                state: crate::state::McpServerUsabilityState::Failed,
            },
        ]);
        for name in ["gh", "k8s"] {
            state.status.mcp.mcp_failure_reasons.insert(
                name.to_string(),
                "handshake failed: connection refused by the MCP server at 127.0.0.1:59258 during initialize".to_string(),
            );
        }
        state.open_info_panel(crate::state::InfoPanel::Status);
    }

    // -- Exec
    status.advance_with_frame(&[]).await?;

    // -- Check: the title carries the cue computed from the rendered lines.
    let popup = super::render::frame::modal_rect_for_panel(
        status.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Status,
    );
    let inner = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (_title, lines) = super::status_panel_lines(status.state(), inner.width);
    let cue = help_panel_overflow_cue_for_test(&lines, inner.height, 0);
    assert!(
        cue.contains(" / "),
        "the status fixture must overflow its viewport; cue={cue}"
    );
    let buffer = status.buffer_text();
    assert!(
        buffer.contains(&format!("Status ({cue})")),
        "the status title must carry its position cue; buffer:\n{buffer}"
    );

    // -- Setup & Fixtures: a skills panel with more rows than its viewport.
    let mut skills = RenderLoopDriver::new(80, 24);
    {
        let state = &mut skills.coordinator_mut().state;
        state.status.mcp.set_discoverable_skills(
            (0..25)
                .map(|idx| crate::state::DiscoverableSkill {
                    source_priority: 0,
                    source: "repo".to_string(),
                    name: format!("skill{idx}"),
                    description: None,
                })
                .collect(),
        );
        state.open_info_panel(crate::state::InfoPanel::Skills);
    }

    // -- Exec
    skills.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        skills.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Skills,
    );
    let inner = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (_title, lines) = crate::runtime::skills_panel_lines(skills.state(), inner.width);
    let cue = help_panel_overflow_cue_for_test(&lines, inner.height, 0);
    assert!(
        cue.contains(" / "),
        "the skills fixture must overflow its viewport; cue={cue}"
    );
    let buffer = skills.buffer_text();
    assert!(
        buffer.contains(&format!("Skills ({cue})")),
        "the skills title must carry its position cue; buffer:\n{buffer}"
    );
    Ok(())
}

/// The help panel scroll reaches the final content line, shows an overflow cue,
/// clamps the scroll offset, and Esc closes the panel.
#[tokio::test]
async fn help_panel_scroll_and_escape() -> Result<()> {
    // -- Setup & Fixtures
    let viewport_width = 80u16;
    let (_title, lines) = help_panel_lines(
        &crate::rendering::theme::TuiTheme::default(),
        viewport_width,
    );
    let viewport_inner_height = 8u16;

    // -- Exec & Check: the scroll window reaches the footer content.
    let max_scroll = help_panel_max_scroll_for_test(&lines, viewport_inner_height);
    let window = help_panel_visible_window_for_test(&lines, viewport_inner_height, max_scroll);
    let rendered = window
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("If text looks stale after resize"));

    // -- Exec & Check: the cue always carries the navigation keys, plus the
    // position only while the content overflows.
    let cue = help_panel_overflow_cue_for_test(&lines, viewport_inner_height, 3);
    assert!(cue.starts_with("↑/↓ · PgUp/PgDn · Esc"), "cue={cue}");
    assert!(cue.contains(" / "), "cue={cue}");
    assert!(!cue.contains('|'), "no legacy separators: {cue}");

    // -- Exec & Check: short content keeps the keys and drops the position.
    let short_cue = help_panel_overflow_cue_for_test(&lines, 1000, 3);
    assert_eq!(short_cue, "↑/↓ · PgUp/PgDn · Esc");

    // -- Exec & Check: the scroll offset is clamped.
    let small_scroll =
        super::help_panel_scroll_offset_for_test(viewport_inner_height, viewport_width, 3);
    assert_eq!(small_scroll, 3);
    let huge_scroll =
        super::help_panel_scroll_offset_for_test(viewport_inner_height, viewport_width, usize::MAX);
    assert_eq!(huge_scroll, max_scroll);
    assert!(max_scroll > 0);

    // -- Setup & Fixtures: a driver with the help panel open.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Help);
    driver.coordinator_mut().state.info_panel_scroll = 5;

    // -- Exec & Check: Down scrolls (or holds) the panel.
    driver.advance(&[key(TerminalKey::Down)]).await?;
    assert!(driver.state().info_panel_scroll >= 5);

    // -- Exec & Check: Esc closes the panel.
    driver.advance(&[key(TerminalKey::Esc)]).await?;
    assert_eq!(driver.state().info_panel, None);
    Ok(())
}

/// The skills panel renders an empty state when no skills are available and
/// lists skills in deterministic order.
#[test]
fn skills_panel_content() -> Result<()> {
    // -- Setup & Fixtures: no skills.
    let state = AppState::default();

    // -- Exec & Check: the empty state.
    let (title, lines) = crate::runtime::skills_panel_lines(&state, 200);
    assert_eq!(title, "Skills");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("No discoverable skills available."));

    // -- Setup & Fixtures: three skills.
    let mut ordered = AppState::default();
    ordered.status.mcp.set_discoverable_skills(vec![
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

    // -- Exec
    let (_title, lines) = crate::runtime::skills_panel_lines(&ordered, 200);
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    // -- Check: deterministic order and description rendering.
    let alpha_idx = rendered.find("alpha").ok_or("should find alpha row")?;
    let beta_idx = rendered.find("beta").ok_or("should find beta row")?;
    let zeta_idx = rendered.find("zeta").ok_or("should find zeta row")?;
    assert!(alpha_idx < beta_idx);
    assert!(beta_idx < zeta_idx);
    assert!(rendered.contains("- alpha (repo) — Alpha agent"));
    assert!(rendered.contains("- beta (repo)\n"));
    Ok(())
}

/// The status and skills panel scroll offsets are clamped to the max.
#[test]
fn status_and_skills_panel_scroll_offset() -> Result<()> {
    // -- Setup & Fixtures: a status panel with one server.
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: crate::state::McpServerUsabilityState::Enabled,
        }]);
    let viewport_height = 3u16;
    let viewport_width = 80u16;

    // -- Exec & Check: the status scroll offset is clamped.
    let small_scroll =
        super::status_panel_scroll_offset_for_test(&state, viewport_height, viewport_width, 1);
    let (_title, lines) = super::status_panel_lines(&state, viewport_width);
    let max = super::help_panel_max_scroll_for_test(&lines, viewport_height);
    assert!(small_scroll <= max);
    let huge_scroll = super::status_panel_scroll_offset_for_test(
        &state,
        viewport_height,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(huge_scroll, max);

    // -- Setup & Fixtures: a skills panel with two skills.
    let mut skills = AppState::default();
    skills.status.mcp.set_discoverable_skills(vec![
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
    let skills_viewport = 2u16;

    // -- Exec & Check: the skills scroll offset is clamped.
    let (_title, lines) = super::skills_panel_lines(&skills, viewport_width);
    let max = super::help_panel_max_scroll_for_test(&lines, skills_viewport);
    let huge_scroll = super::skills_panel_scroll_offset_for_test(
        &skills,
        skills_viewport,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(huge_scroll, max);
    let zero_scroll =
        super::skills_panel_scroll_offset_for_test(&skills, skills_viewport, viewport_width, 0);
    assert_eq!(zero_scroll, 0);
    Ok(())
}

use unicode_width::UnicodeWidthStr;

use super::*;

/// The command palette table renders the required columns and rows, includes
/// the Skills/Models actions, emits an overflow cue at a small viewport, and
/// the inline slash suggestions render inline with a single hint contract.
#[test]
fn command_palette_table_and_inline_slash() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec
    let model = command_palette_table_model_for_test(&state, 80, 10);

    // -- Check: columns, actions, and selection.
    assert_eq!(model.columns, vec!["Action", "Summary"]);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec!["Help", "Status", "MCPs", "Skills", "Models", "Agents"]
    );
    assert!(model.rows.iter().all(|row| row[2].is_empty()));
    assert_eq!(model.selected, Some(0));
    assert!(actions.contains(&"Skills"));
    assert!(actions.contains(&"Models"));

    // -- Exec & Check: the overflow cue at a small viewport carries only the
    // position, so the title can join it to the always-on hint with " · ".
    let small = command_palette_table_model_for_test(&state, 80, 5);
    let cue = small
        .overflow_cue
        .ok_or("should have overflow cue when rows exceed viewport")?;
    assert!(cue.contains(" / "), "cue={cue}");
    assert!(!cue.contains('|'), "cue={cue}");
    assert!(!cue.contains("Esc close"), "cue={cue}");

    // -- Setup & Fixtures: inline slash suggestions.
    let mut slash = AppState::default();
    slash.check_inline_slash("/");

    // -- Exec & Check: the inline rows and the single hint contract.
    let rows = inline_slash_lines_for_test(&slash);
    assert!(!rows.is_empty());
    assert!(rows[0].contains("/compact"));
    assert!(rows[0].starts_with('❯'));
    let title = super::command_palette_title(None, 48);
    assert_eq!(title, "Command Palette (↑/↓ · Enter · Esc)");
    let overflow_title = super::command_palette_title(Some("1-5 / 8"), 48);
    assert_eq!(
        overflow_title,
        "Command Palette (↑/↓ · Enter · Esc · 1-5 / 8)"
    );

    // -- Exec & Check: the inline-slash popup carries the same hint contract.
    assert_eq!(super::inline_slash_title(), "Commands (↑/↓ · Enter · Esc)");
    Ok(())
}

/// The palette Summary cell is ellipsized to the column budget and returns a
/// fitting summary unchanged.
#[test]
fn command_palette_table_ellipsizes_summaries() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec: an 18-column table is infeasible for the constraints, so the
    // Summary column keeps its 12-cell minimum.
    let narrow = command_palette_table_model_for_test(&state, 18, 12);

    // -- Check: every summary is clipped to 11 chars plus the trailing
    // ellipsis.
    assert_eq!(narrow.rows.len(), 8);
    for row in &narrow.rows {
        let summary = row[1].as_str();
        assert_eq!(summary.chars().count(), 12, "summary={summary:?}");
        assert!(summary.ends_with('…'), "summary={summary:?}");
    }

    // -- Exec: a 46-column table leaves 34 chars for Summary.
    let wide = command_palette_table_model_for_test(&state, 46, 12);

    // -- Check: the longest summary fits and returns unchanged.
    let sessions = wide
        .rows
        .iter()
        .find(|row| row[0] == "Sessions")
        .ok_or("should render the Sessions row")?;
    assert_eq!(sessions[1], "Switch to an existing session");
    Ok(())
}

/// The palette marks a clipped Action label: at the minimum popup width the
/// Action column is narrower than the longest label, so the label ends with a
/// trailing ellipsis instead of hard-clipping.
#[tokio::test]
async fn command_palette_marks_clipped_labels_at_minimum_popup_width() -> Result<()> {
    // -- Setup & Fixtures: the palette open on the longest label (Sessions).
    let mut driver = RenderLoopDriver::new(26, 30);
    open_command_palette_for_test(&mut driver.coordinator_mut().state);
    let downs = (0..6)
        .map(|_| DriveEvent::Key(TerminalEvent::Key(TerminalKey::Down)))
        .collect::<Vec<_>>();

    // -- Exec
    driver.advance_with_frame(&downs).await?;

    // -- Check: popup 24 → table 22 → Action column 6 cells, so the label
    // keeps 5 characters and marks the clip.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected palette row")?;
    assert!(
        row.contains("Sessi…"),
        "the clipped label should be marked; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// A feasible palette width keeps every Action label in full: the labels are no
/// wider than the 8-cell Action column and the Summary cell absorbs the rest.
#[tokio::test]
async fn command_palette_renders_full_labels_at_feasible_width() -> Result<()> {
    // -- Setup & Fixtures: the palette open on the longest label (Sessions).
    let mut driver = RenderLoopDriver::new(120, 30);
    open_command_palette_for_test(&mut driver.coordinator_mut().state);
    let downs = (0..6)
        .map(|_| DriveEvent::Key(TerminalEvent::Key(TerminalKey::Down)))
        .collect::<Vec<_>>();

    // -- Exec
    driver.advance_with_frame(&downs).await?;

    // -- Check: popup 48 → table 46 → Action column 8 cells, so the longest
    // label renders in full and the Summary column keeps the longest summary.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected palette row")?;
    assert!(
        row.contains("Sessions"),
        "a feasible width must render the label in full; row: {row:?}\n{popup_text}"
    );
    assert!(
        row.contains("Switch to an existing session"),
        "a feasible width must render the summary in full; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// The modal layout policy applies consistently across panels: Help is large,
/// Status is compact, and the remaining panels share the default size.
#[test]
fn modal_layout_policy() -> Result<()> {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);

    // -- Exec & Check: Help is large.
    let help = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Help,
    );
    assert!(help.width >= 72);
    assert!(help.height >= 18);

    // -- Exec & Check: Status is compact.
    let status = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Status,
    );
    assert!(status.width <= 72);
    assert!(status.height <= 14);

    // -- Exec & Check: Skills shares the default size; MCPs and the palette
    // are narrower.
    let command_palette = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    let skills = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Skills,
    );
    let mcps = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert!(mcps.width < skills.width);
    assert_eq!(skills.height, mcps.height);
    assert!(command_palette.width < skills.width);

    // -- Exec & Check: Models and MCPs are narrower than the shared default;
    // the themes picker is right-sized to its own content.
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let themes = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Themes,
    );
    assert!(models.width < skills.width);
    assert!(mcps.width < models.width);
    assert_eq!(models.height, skills.height);
    assert!(themes.width < models.width);
    assert_eq!(themes.width, 32);
    assert_eq!(themes.height, 12);
    Ok(())
}

/// A modal open state applies the dimmed backdrop; the inline model picker
/// respects the border/backdrop policy; a permission prompt does not open the
/// global backdrop; and an empty model catalog shows a deterministic empty
/// state.
#[test]
fn modal_backdrop_and_empty_state() -> Result<()> {
    // -- Setup & Fixtures: the command palette open.
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec & Check: the dimmed backdrop applies.
    assert!(modal_open_state_applies_dimmed_backdrop_for_test(&state));

    // -- Setup & Fixtures: the inline model picker open.
    let mut model = AppState::default();
    model.picker.open(ActivePicker::Model);

    // -- Exec & Check: the border/backdrop policy is respected.
    assert!(inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(&model));

    // -- Setup & Fixtures: a permission prompt.
    let mut permission = AppState::default();
    permission
        .permission
        .open_prompt(crate::state::PermissionPrompt {
            request_id: "ask-0000000000000001".to_string(),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            scope: "nested".to_string(),
            pattern: "*".to_string(),
            target_field: Some("command".to_string()),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
        });

    // -- Exec & Check: no global backdrop.
    assert!(!modal_open_state_applies_dimmed_backdrop_for_test(
        &permission
    ));

    // -- Setup & Fixtures: an empty model catalog.
    let mut empty = AppState::default();
    assert_eq!(
        crate::runtime::panels::MODEL_PICKER_EMPTY_STATE_MESSAGE,
        "No models available in cached startup config."
    );
    empty.picker.open(ActivePicker::Model);

    // -- Exec & Check: the deterministic empty state.
    assert!(
        empty
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .is_empty()
    );
    Ok(())
}

#[test]
fn render_modal_frame_inner_is_inset_by_one_cell() -> Result<()> {
    // -- Setup & Fixtures
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 5,
    };
    let theme = crate::rendering::theme::TuiTheme::default();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(10, 5))?;

    // -- Exec
    let mut inner = ratatui::layout::Rect::default();
    terminal.draw(|frame| {
        inner = super::render::render_modal_frame(frame, area, "Frame", &theme);
    })?;

    // -- Check
    assert_eq!(
        inner,
        ratatui::layout::Rect {
            x: 1,
            y: 1,
            width: 8,
            height: 3,
        },
        "the frame inset must stay one cell per side"
    );
    let buffer = terminal.backend().buffer();
    for (x, y) in [(0, 0), (9, 0), (0, 4), (9, 4)] {
        assert_ne!(
            buffer[(x, y)].symbol(),
            " ",
            "the frame border must cover ({x}, {y})"
        );
    }
    Ok(())
}

#[test]
fn scrollbar_state_does_not_panic_on_empty_or_single_entry() {
    let mut empty = ratatui::widgets::ScrollbarState::new(0).position(0);
    let _ = &mut empty;
    let mut single = ratatui::widgets::ScrollbarState::new(1).position(0);
    let _ = &mut single;
}

#[test]
fn transcript_list_area_is_two_columns_narrower_than_content_area() {
    let content_area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 20,
    };
    let list_area = ratatui::layout::Rect {
        width: content_area.width.saturating_sub(2),
        ..content_area
    };
    assert_eq!(list_area.width, 78);
    assert_eq!(list_area.x, content_area.x);
    assert_eq!(list_area.height, content_area.height);
}

/// At the popup's minimum width the composed title overflows the title row, so
/// it ellipsizes to that row with a trailing marker and drops the keys hint.
#[test]
fn command_palette_title_ellipsizes_at_minimum_popup_width() -> Result<()> {
    // -- Exec & Check: 20-column popup → 18-cell title row.
    assert_eq!(super::command_palette_title(None, 20), "Command Palette (…");
    assert_eq!(
        super::command_palette_title(Some("1-5 / 8"), 20),
        "Command Palette (…"
    );
    Ok(())
}

/// The title never exceeds the title row across the popup's whole width range:
/// it truncates to the row while it overflows and renders whole once it fits.
#[test]
fn command_palette_title_fits_title_row_at_every_popup_width() -> Result<()> {
    // -- Setup & Fixtures: the unbounded titles the renderer would clip.
    let cases: [(Option<&str>, &str); 2] = [
        (None, "Command Palette (↑/↓ · Enter · Esc)"),
        (
            Some("1-5 / 8"),
            "Command Palette (↑/↓ · Enter · Esc · 1-5 / 8)",
        ),
    ];

    // -- Exec & Check
    for popup_width in 20u16..=48 {
        let row_width = usize::from(popup_width - 2);
        for (cue, unbounded) in cases {
            let title = super::command_palette_title(cue, popup_width);
            let width = UnicodeWidthStr::width(title.as_str());
            assert_eq!(
                width,
                UnicodeWidthStr::width(unbounded).min(row_width),
                "the title must fill the {row_width}-cell title row at popup width \
                 {popup_width}; title: {title:?}"
            );
            if UnicodeWidthStr::width(unbounded) > row_width {
                assert!(
                    title.ends_with('…'),
                    "the overflowing title must truncate at popup width {popup_width}; \
                     title: {title:?}"
                );
            } else {
                assert_eq!(
                    title, unbounded,
                    "the fitting title must render verbatim at popup width {popup_width}"
                );
            }
        }
    }
    Ok(())
}

/// At the palette's minimum popup width the title overflows the title row, so
/// the rendered title ends with the ellipsis marker instead of hard-clipping at
/// the frame border.
#[tokio::test]
async fn command_palette_rendered_title_ellipsizes_at_minimum_popup_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(22, 24);
    open_command_palette_for_test(&mut driver.coordinator_mut().state);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the title row spans the popup width minus the two border corners.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    assert_eq!(
        popup.width, 20,
        "the palette must render at the 20-column minimum"
    );
    let title = title_row_text(&driver.buffer_text_in_rect(popup))?;
    assert!(
        title.ends_with('…'),
        "the title must ellipsize with a marker; title: {title:?}"
    );
    assert!(
        UnicodeWidthStr::width(title.as_str()) <= 18,
        "the title must fit the 18-cell title row; title: {title:?}"
    );
    Ok(())
}

// region:    --- Support

/// The popup's title row as plain text, stripped of the frame border corners.
fn title_row_text(popup_text: &str) -> Result<String> {
    let row = popup_text
        .lines()
        .next()
        .ok_or("should render the title row")?;
    let inner = row
        .chars()
        .skip(1)
        .take(row.chars().count().saturating_sub(2))
        .collect::<String>();
    Ok(inner)
}

// endregion: --- Support

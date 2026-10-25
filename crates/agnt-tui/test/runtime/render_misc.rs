use super::*;

/// The main pane vertical split reserves the status target height and the
/// input minimum height, leaving the transcript the remaining space with no
/// overlap or bottom cutoff.
#[test]
fn main_pane_vertical_split_layout() -> Result<()> {
    use crate::rendering::layout::INPUT_MIN_HEIGHT;
    use crate::runtime::render::frame_test::{STATUS_TARGET_HEIGHT, main_pane_rects_for_height};

    // -- Exec & Check: a 10-row main pane.
    let (header, transcript, input, status) = RuntimeCoordinator::main_pane_rects_for_height(10);
    assert_eq!(header.height, 0);
    assert!(transcript.height > 0);
    assert_eq!(status.height, STATUS_TARGET_HEIGHT);
    assert_eq!(transcript.y + transcript.height, input.y);
    assert_eq!(input.y + input.height, status.y);
    assert_eq!(status.y + status.height, 10);

    // -- Exec & Check: a 40-row main pane.
    let main_height = 40u16;
    let (header, transcript, input, status) = main_pane_rects_for_height(main_height);
    assert_eq!(header.height, 0);
    assert_eq!(status.height, STATUS_TARGET_HEIGHT);
    assert_eq!(input.height, INPUT_MIN_HEIGHT);
    assert_eq!(
        transcript.height,
        main_height - INPUT_MIN_HEIGHT - STATUS_TARGET_HEIGHT
    );
    Ok(())
}

/// `single_line_visual_row_count` counts wrapped rows, treating an empty line
/// and a zero width as one row.
#[test]
fn single_line_visual_row_count_matrix() -> Result<()> {
    use ratatui::text::Span;

    // -- Setup & Fixtures
    let multi_span = ratatui::text::Line::from(vec![
        Span::raw("a".repeat(50)),
        Span::raw("b".repeat(50)),
        Span::raw("c".repeat(50)),
    ]);
    let cases: [(&str, ratatui::text::Line, usize, usize); 5] = [
        ("short line", ratatui::text::Line::from("1234567890"), 80, 1),
        (
            "wider than viewport",
            ratatui::text::Line::from("x".repeat(200)),
            80,
            3,
        ),
        ("multi span", multi_span, 80, 2),
        ("empty line", ratatui::text::Line::from(""), 80, 1),
        ("width zero", ratatui::text::Line::from("hello"), 0, 1),
    ];

    // -- Exec & Check
    for (label, line, width, expected) in cases {
        assert_eq!(
            super::render::single_line_visual_row_count(&line, width),
            expected,
            "{label}"
        );
    }
    Ok(())
}

/// The height index is computed on a new entry, sums measure per block plus
/// separators, and is invalidated on clear.
#[test]
fn height_index_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = crate::state::AppState::default();
    push_message_line(&mut state, MessageRole::User, "hello");

    // -- Exec
    state.transcript.rebuild_height_index(80);

    // -- Check: computed on a new entry.
    assert!(state.transcript.height_index_valid_for(80));
    assert!(state.transcript.total_visual_rows() >= 1);

    // -- Setup & Fixtures: three blocks.
    let mut summed = crate::state::AppState::default();
    push_message_line(&mut summed, MessageRole::User, "a");
    push_message_line(&mut summed, MessageRole::Assistant, "b\nc\nd");
    push_message_line(&mut summed, MessageRole::User, "e");

    // -- Exec
    summed.transcript.rebuild_height_index(80);

    // -- Check: measure sum + separators + trailing row.
    let measured: usize = summed
        .transcript
        .blocks()
        .iter()
        .map(|block| crate::tui_renderer::measure(block, 80, &TuiTheme::default()))
        .sum();
    assert_eq!(summed.transcript.total_visual_rows(), measured + 4 + 1);
    assert_eq!(summed.transcript.total_visual_rows(), 10);

    // -- Setup & Fixtures: a built index.
    let mut cleared = crate::state::AppState::default();
    push_message_line(&mut cleared, MessageRole::User, "hello");
    cleared.transcript.rebuild_height_index(80);
    assert!(cleared.transcript.height_index_valid_for(80));

    // -- Exec
    cleared.clear_transcript();

    // -- Check: invalidated on clear.
    assert!(!cleared.transcript.height_index_valid_for(80));
    assert_eq!(cleared.transcript.total_visual_rows(), 0);
    Ok(())
}

/// The startup logo is pushed on demand, never during hydration, and a single
/// logo leaves the transcript shorter than the viewport.
#[test]
fn startup_logo_and_bottom_align() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    coordinator.state.push_startup_logo();

    // -- Check: the logo is pushed.
    let blocks = coordinator.state.transcript.blocks();
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].source, BlockSource::Banner { .. }));

    // -- Setup & Fixtures: hydration with no messages.
    let mut hydrating = RuntimeCoordinator::new(120, 40, Some(false));
    let messages: Vec<UiMessageSnapshot> = vec![];

    // -- Exec
    hydrating.hydrate_transcript_from_messages(messages, None);

    // -- Check: hydration must not push a logo.
    let has_logo = hydrating
        .state
        .transcript
        .blocks()
        .iter()
        .any(|b| matches!(b.source, BlockSource::Banner { .. }));
    assert!(!has_logo);

    // -- Setup & Fixtures: a single logo on a 40-row terminal.
    let mut bottom = RuntimeCoordinator::new(120, 40, Some(false));
    bottom.state.push_startup_logo();
    bottom.state.transcript.invalidate_height_index();

    // -- Exec
    bottom.state.transcript.rebuild_height_index(120);

    // -- Check: the transcript is shorter than the viewport.
    let total = bottom.state.transcript.total_visual_rows();
    let vp = bottom.state.scroll.viewport_height;
    assert!(
        total < vp || vp == 0,
        "expected total_visual_rows ({total}) < viewport_height ({vp}) for single logo"
    );
    Ok(())
}

/// The status section paints the message above the lanes, and the lanes below
/// the input box bottom border, for both wrapping and fitting model identities.
#[tokio::test]
async fn status_section_lane_layout() -> Result<()> {
    // -- Setup & Fixtures: a wrapping identity with a message.
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/status-lane-wrap");

    let mut driver = RenderLoopDriver::new(60, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo.clone()));
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity =
        "openai/gpt-5-ultra-long-model-identity-that-forces-wrap-0123456789".to_string();
    driver
        .coordinator_mut()
        .state
        .status
        .message
        .set_message("status-message-sentinel");

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: message < left < right.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let msg_row = lines
        .iter()
        .position(|l| l.contains("status-message-sentinel"))
        .ok_or("should find message row")?;
    let left_row = lines
        .iter()
        .position(|l| l.contains("0123456789"))
        .ok_or("should find left lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert!(
        msg_row < left_row && left_row < right_row,
        "wrap+message must paint message < left < right, got msg={msg_row} left={left_row} right={right_row}\n{buffer}",
    );

    // -- Setup & Fixtures: a fitting identity with a message.
    let mut fit = RenderLoopDriver::new(120, 30);
    fit.coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo.clone()));
    fit.coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();
    fit.coordinator_mut()
        .state
        .status
        .message
        .set_message("status-message-sentinel");

    // -- Exec
    fit.advance_with_frame(&[]).await?;

    // -- Check: message above the lane; left and right on the same row.
    let buffer = fit.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let msg_row = lines
        .iter()
        .position(|l| l.contains("status-message-sentinel"))
        .ok_or("should find message row")?;
    let lane_row = lines
        .iter()
        .position(|l| l.contains("gpt-5"))
        .ok_or("should find lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert!(
        msg_row < lane_row,
        "fit+message must paint message above the lane, got msg={msg_row} lane={lane_row}\n{buffer}",
    );
    assert_eq!(lane_row, right_row);

    // -- Setup & Fixtures: a wrapping identity with no message.
    let mut wrap_no_msg = RenderLoopDriver::new(60, 30);
    wrap_no_msg
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo.clone()));
    wrap_no_msg
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity =
        "openai/gpt-5-ultra-long-model-identity-that-forces-wrap-0123456789".to_string();

    // -- Exec
    wrap_no_msg.advance_with_frame(&[]).await?;

    // -- Check: lanes immediately below the input box bottom border.
    let buffer = wrap_no_msg.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let border_row = lines
        .iter()
        .rposition(|l| l.contains('╰'))
        .ok_or("should find input box bottom border row")?;
    let left_row = lines
        .iter()
        .position(|l| l.contains("0123456789"))
        .ok_or("should find left lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert_eq!(left_row, border_row + 1);
    assert_eq!(right_row, left_row + 1);

    // -- Setup & Fixtures: a fitting identity with no message.
    let mut fit_no_msg = RenderLoopDriver::new(120, 30);
    fit_no_msg
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
    fit_no_msg
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();

    // -- Exec
    fit_no_msg.advance_with_frame(&[]).await?;

    // -- Check: a single lane immediately below the input box bottom border.
    let buffer = fit_no_msg.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let border_row = lines
        .iter()
        .rposition(|l| l.contains('╰'))
        .ok_or("should find input box bottom border row")?;
    let lane_row = lines
        .iter()
        .position(|l| l.contains("gpt-5"))
        .ok_or("should find lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert_eq!(lane_row, border_row + 1);
    assert_eq!(lane_row, right_row);
    Ok(())
}

#[tokio::test]
async fn every_rendered_cell_has_an_explicit_background() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    if let Some((x, y)) = driver.first_cell_without_bg() {
        return Err(format!(
            "cell ({x},{y}) has no explicit background after render; the opaque theme surface must paint every cell\n{}",
            driver.buffer_text()
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn theme_picker_popup_does_not_bleed_transcript_glyphs() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    push_message_line(
        &mut driver.coordinator_mut().state,
        MessageRole::User,
        "bleed-sentinel",
    );
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Theme,
        vec![PickerOption {
            id: "CatppuccinMocha".to_string(),
            display: "Catppuccin Mocha".to_string(),
            search_text: "Catppuccin Mocha".to_string(),
            sort_key: Vec::new(),
            payload: PickerPayload::Theme,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Theme);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    assert!(
        !popup_text.contains("bleed-sentinel"),
        "theme picker popup must not show stale transcript glyphs; got:\n{popup_text}",
    );
    Ok(())
}

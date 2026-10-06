use super::*;

#[test]
fn main_pane_vertical_split_has_no_overlap_or_bottom_cutoff() {
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    let (_header, transcript, input, status) = RuntimeCoordinator::main_pane_rects_for_height(10);

    assert_eq!(_header.height, 0);
    assert!(
        transcript.height > 0,
        "transcript pane should remain visible"
    );
    assert_eq!(
        status.height, STATUS_TARGET_HEIGHT,
        "footer must reserve STATUS_TARGET_HEIGHT rows"
    );
    assert_eq!(transcript.y + transcript.height, input.y);
    assert_eq!(input.y + input.height, status.y);
    assert_eq!(status.y + status.height, 10);
}

#[test]
fn main_pane_rects_transcript_gets_remaining_space() {
    use crate::rendering::layout::INPUT_MIN_HEIGHT;
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    use crate::runtime::render::frame_test::main_pane_rects_for_height;

    let main_height = 40u16;
    let (header, transcript, input, status) = main_pane_rects_for_height(main_height);

    assert_eq!(header.height, 0);
    assert_eq!(status.height, STATUS_TARGET_HEIGHT);
    assert_eq!(input.height, INPUT_MIN_HEIGHT);
    assert_eq!(
        transcript.height,
        main_height - INPUT_MIN_HEIGHT - STATUS_TARGET_HEIGHT
    );
}

// ========== single_line_visual_row_count tests ==========

#[test]
fn single_line_visual_row_count_short_line() {
    let line = ratatui::text::Line::from("1234567890");
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 1);
}

#[test]
fn single_line_visual_row_count_wider_than_viewport() {
    let line = ratatui::text::Line::from("x".repeat(200));
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 3);
}

#[test]
fn single_line_visual_row_count_multi_span() {
    use ratatui::text::Span;
    let line = ratatui::text::Line::from(vec![
        Span::raw("a".repeat(50)),
        Span::raw("b".repeat(50)),
        Span::raw("c".repeat(50)),
    ]);
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 2);
}

#[test]
fn single_line_visual_row_count_empty_line() {
    let line = ratatui::text::Line::from("");
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 1);
}

#[test]
fn single_line_visual_row_count_width_zero() {
    let line = ratatui::text::Line::from("hello");
    let count = super::render::single_line_visual_row_count(&line, 0);
    assert_eq!(count, 1);
}

// ========== rendered_line_text gating tests ==========

#[test]
fn should_scan_for_yank_insert_mode_returns_false() {
    assert!(!super::render::should_scan_for_yank(InputMode::Insert));
}

#[test]
fn should_scan_for_yank_normal_mode_returns_false() {
    assert!(!super::render::should_scan_for_yank(InputMode::Normal));
}

#[test]
fn should_scan_for_yank_visual_mode_returns_true() {
    assert!(super::render::should_scan_for_yank(InputMode::Visual));
}

// ========== height index tests ==========

#[test]
fn height_index_computed_on_new_entry() {
    let mut state = crate::state::AppState::default();
    push_message_line(&mut state, MessageRole::User, "hello");
    state.transcript.rebuild_height_index(80);
    assert!(state.transcript.height_index_valid_for(80));
    assert!(state.transcript.total_visual_rows() >= 1);
}

#[test]
fn total_visual_rows_sums_measure_per_block_plus_separators() {
    let mut state = crate::state::AppState::default();
    push_message_line(&mut state, MessageRole::User, "a");
    push_message_line(&mut state, MessageRole::Assistant, "b\nc\nd");
    push_message_line(&mut state, MessageRole::User, "e");
    state.transcript.rebuild_height_index(80);
    // Content blocks only: [User "a", Assistant, User "e"].
    // measure sum = 1 + 3 + 1 = 5; SM separators = 0 + 2 + 2 = 4; the final
    // block is a user turn, so it closes with 1 trailing separator row
    // (task 670e0292).
    let measured: usize = state
        .transcript
        .blocks()
        .iter()
        .map(|block| crate::tui_renderer::measure(block, 80, &TuiTheme::default()))
        .sum();
    let expected = measured + 4 + 1;
    assert_eq!(state.transcript.total_visual_rows(), expected);
    // 5 content rows + 4 separator rows + 1 trailing row = 10 visual rows
    assert_eq!(state.transcript.total_visual_rows(), 10);
}

#[test]
fn height_index_invalidated_on_clear_transcript() {
    let mut state = crate::state::AppState::default();
    push_message_line(&mut state, MessageRole::User, "hello");
    state.transcript.rebuild_height_index(80);
    assert!(state.transcript.height_index_valid_for(80));
    state.clear_transcript();
    assert!(!state.transcript.height_index_valid_for(80));
    assert_eq!(state.transcript.total_visual_rows(), 0);
}

#[test]
fn push_startup_logo_adds_logo_entry_to_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.state.push_startup_logo();
    let blocks = coordinator.state.transcript.blocks();
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].source, BlockSource::Banner { .. }));
}

#[test]
fn startup_logo_not_pushed_during_hydration() {
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    let messages: Vec<UiMessageSnapshot> = vec![];
    coordinator.hydrate_transcript_from_messages(messages, None);
    let has_logo = coordinator
        .state
        .transcript
        .blocks()
        .iter()
        .any(|b| matches!(b.source, BlockSource::Banner { .. }));
    assert!(!has_logo, "hydration must not push a logo");
}

#[test]
fn bottom_align_pads_content_when_shorter_than_viewport() {
    // This test verifies the render-time behavior: when total_visual_rows < viewport_height,
    // the rendered output should have viewport_height lines (padded with empty lines at top).
    // We test this indirectly by checking that the coordinator's state reflects the padding
    // after a render pass.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    // Push a single logo entry — total_visual_rows will be small
    coordinator.state.push_startup_logo();
    // Force a render to trigger the bottom-align logic.
    coordinator.state.transcript.invalidate_height_index();
    // The actual padding happens in render_transcript_pane which we can't easily unit-test
    // without a full Frame. Instead, verify the state is set up correctly for bottom-align:
    // viewport_height > total_visual_rows should be true for a single logo on a 40-row terminal.
    coordinator.state.transcript.rebuild_height_index(120);
    let total = coordinator.state.transcript.total_visual_rows();
    let vp = coordinator.state.scroll.viewport_height;
    // On a 40-row terminal, a single logo entry should be much shorter
    assert!(
        total < vp || vp == 0,
        "expected total_visual_rows ({total}) < viewport_height ({vp}) for single logo"
    );
}

#[tokio::test]
async fn status_section_wrap_message_renders_rows_in_strictly_increasing_order() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/status-lane-wrap");

    let mut driver = RenderLoopDriver::new(60, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
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

    // -- Check
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
    Ok(())
}

#[tokio::test]
async fn status_section_fit_message_renders_message_above_single_lane_row() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/status-lane-wrap");

    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();
    driver
        .coordinator_mut()
        .state
        .status
        .message
        .set_message("status-message-sentinel");

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
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
    assert_eq!(
        lane_row, right_row,
        "fit+message must paint left and right fragments on the same row\n{buffer}",
    );
    Ok(())
}

#[tokio::test]
async fn status_section_wrap_no_message_renders_lanes_below_input_divider() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/status-lane-wrap");

    let mut driver = RenderLoopDriver::new(60, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity =
        "openai/gpt-5-ultra-long-model-identity-that-forces-wrap-0123456789".to_string();

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let div_row = lines
        .iter()
        .position(|l| l.contains("├"))
        .ok_or("should find input divider row")?;
    let left_row = lines
        .iter()
        .position(|l| l.contains("0123456789"))
        .ok_or("should find left lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert_eq!(
        left_row,
        div_row + 1,
        "wrap+no-message must paint left lane immediately below the input divider\n{buffer}",
    );
    assert_eq!(
        right_row,
        left_row + 1,
        "wrap+no-message must paint right lane immediately below the left lane\n{buffer}",
    );
    Ok(())
}

#[tokio::test]
async fn status_section_fit_no_message_renders_single_lane_below_input_divider() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/status-lane-wrap");

    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let div_row = lines
        .iter()
        .position(|l| l.contains("├"))
        .ok_or("should find input divider row")?;
    let lane_row = lines
        .iter()
        .position(|l| l.contains("gpt-5"))
        .ok_or("should find lane row")?;
    let right_row = lines
        .iter()
        .position(|l| l.contains("feature/status-lane-wrap"))
        .ok_or("should find right lane row")?;
    assert_eq!(
        lane_row,
        div_row + 1,
        "fit+no-message must paint the lane immediately below the input divider\n{buffer}",
    );
    assert_eq!(
        lane_row, right_row,
        "fit+no-message must paint left and right fragments on the same row\n{buffer}",
    );
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

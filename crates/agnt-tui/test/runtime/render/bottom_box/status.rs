//! Footer status rows: shades, flush placement, and alignment.

use super::bottom_box_support::*;
use crate::runtime::status::test::init_repo_with_branch;

/// WHEN a frame renders with an empty queue, THE input interior and the status
/// rows SHALL paint `base`, and THE status SHALL sit flush below the input box
/// bottom border.
#[tokio::test]
async fn test_bottom_box_empty_queue_paints_base_interior_and_footer_status_rows() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let theme = driver.coordinator().theme.clone();
    let top = input_top_row(&driver)?;
    let input_h = driver.coordinator().input_height;
    let bottom = top + 1 + input_h;
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    assert!(
        lines[bottom as usize].contains('╰'),
        "input box bottom border must sit below the interior\n{buffer}"
    );
    assert_eq!(
        cell_bg(&driver, 5, top + 1),
        Some(theme.base),
        "input interior must paint base"
    );
    let status_y = bottom + 1;
    assert_eq!(
        cell_bg(&driver, 1, status_y),
        Some(theme.base),
        "status row must paint base at the left edge"
    );
    assert_eq!(
        cell_bg(&driver, 118, status_y),
        Some(theme.base),
        "status row must paint base at the right edge"
    );
    assert_ne!(
        cell_bg(&driver, 5, top.saturating_sub(1)),
        Some(theme.row_user_bg),
        "an empty queue must not paint a stack row"
    );
    Ok(())
}

/// WHEN a frame renders, THE status content SHALL align with the input box
/// interior: the left lane starts at the indicator column and the right lane
/// ends one column left of the right frame column.
#[tokio::test]
async fn test_bottom_box_status_content_aligns_with_input_box_interior() -> Result<()> {
    // -- Setup & Fixtures: a repo branch so the right lane renders.
    let temp_dir = tempfile::TempDir::new()?;
    let repo = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    init_repo_with_branch(&repo, "feature/status-align");
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

    // -- Check: derive the bottom rect from the input box border row.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let border_row = lines
        .iter()
        .position(|line| line.contains('╭'))
        .ok_or("should find the input box top border row")?;
    let border = lines[border_row];
    let rect_x = border
        .chars()
        .position(|c| c == '╭')
        .ok_or("should find the left border glyph")? as u16;
    let rect_right = border
        .chars()
        .position(|c| c == '╮')
        .ok_or("should find the right border glyph")? as u16;
    let rect_width = rect_right - rect_x + 1;

    let left_row = lines
        .iter()
        .position(|line| line.contains("gpt-5"))
        .ok_or("should find the left lane row")?;
    let right_row = lines
        .iter()
        .position(|line| line.contains("feature/status-align"))
        .ok_or("should find the right lane row")?;

    let left_first = lines[left_row]
        .chars()
        .position(|c| c != ' ')
        .ok_or("left lane must paint a glyph")? as u16;
    let right_chars: Vec<char> = lines[right_row].chars().collect();
    let right_last_char = right_chars
        .iter()
        .rposition(|c| *c != ' ')
        .ok_or("right lane must paint a glyph")?;
    let right_last_col: usize = right_chars
        .iter()
        .take(right_last_char)
        .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
        .sum();

    assert_eq!(
        left_first,
        rect_x + 1,
        "left lane must start at the input box indicator column"
    );
    assert_eq!(
        right_last_col as u16,
        rect_x + rect_width - 2,
        "right lane must end one column left of the input box right frame column"
    );
    Ok(())
}

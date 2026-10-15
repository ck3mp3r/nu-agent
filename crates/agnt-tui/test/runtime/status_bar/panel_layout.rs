use super::*;

/// The status panel targets three rows; the left content carries the model
/// identity and, when tokens are known, the token count and context bar.
#[test]
fn status_panel_layout_and_left_content() -> Result<()> {
    // -- Exec & Check: the target height is three.
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    assert_eq!(STATUS_TARGET_HEIGHT, 3);

    // -- Exec & Check: the left content contains the model.
    let mut state = AppState::default();
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut state, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("openai/gpt-4"));

    // -- Exec & Check: the left content contains tokens and the context bar.
    let mut tokens = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    tokens
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut tokens, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("250"));
    assert!(text.contains("25%"));
    assert!(text.contains('┃'));
    Ok(())
}

/// The status right content shows the branch or the cwd, is none when neither
/// is present, and can exceed a narrow width to trigger overflow.
#[test]
fn status_right_content_contract() -> Result<()> {
    // -- Exec & Check: the branch is shown.
    let line = crate::runtime::status_right_content_for_test(Some("main"), None);
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("main"));

    // -- Exec & Check: none when no branch and no cwd.
    assert!(crate::runtime::status_right_content_for_test(None, None).is_none());

    // -- Exec & Check: the cwd is shown.
    let cwd = std::path::Path::new("/home/user/projects/my-project");
    let line = crate::runtime::status_right_content_for_test(None, Some(cwd));
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("my-project"));

    // -- Exec & Check: a long branch + cwd exceeds a narrow width.
    let narrow_width: u16 = 20;
    let branch = "feature/my-very-long-branch";
    let cwd = std::path::Path::new("/home/user/projects/deep/nested/dir");
    let right_line = crate::runtime::status_right_content_for_test(Some(branch), Some(cwd));
    let right_line = right_line.ok_or("should have right content line")?;
    let right_width: u16 = right_line
        .spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16)
        .sum();
    assert!(
        right_width > narrow_width,
        "right_width={right_width} must exceed narrow_width={narrow_width} to exercise overflow"
    );
    Ok(())
}

use super::*;

#[test]
fn user_block_needs_user_bg() {
    let blocks = vec![user_block()];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn assistant_block_does_not_need_user_bg() {
    let blocks = vec![assistant_block()];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

#[test]
fn spacer_after_user_needs_user_bg() {
    let blocks = vec![user_block(), spacer_block()];
    assert_eq!(
        full_width_background(&blocks, 1, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_before_user_needs_user_bg() {
    let blocks = vec![spacer_block(), user_block()];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_between_two_users_needs_user_bg() {
    let blocks = vec![user_block(), spacer_block(), user_block()];
    assert_eq!(
        full_width_background(&blocks, 1, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_not_adjacent_to_user_does_not_need_user_bg() {
    let blocks = vec![assistant_block(), spacer_block(), assistant_block()];
    assert_eq!(full_width_background(&blocks, 1, FULL_BG, CODE_BG), None);
}

#[test]
fn out_of_range_block_does_not_need_user_bg() {
    let blocks = vec![user_block()];
    assert_eq!(full_width_background(&blocks, 5, FULL_BG, CODE_BG), None);
}

#[test]
fn tool_block_does_not_need_user_bg() {
    let blocks = vec![tool_block(Fill::None)];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

// ── full_width_background: Fill::Code ───────────────────────────────────────

#[test]
fn fill_code_block_needs_code_bg() {
    let blocks = vec![tool_block(Fill::Code)];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(CODE_BG)
    );
}

#[test]
fn fill_none_block_does_not_need_code_bg() {
    let blocks = vec![tool_block(Fill::None)];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

#[test]
fn out_of_range_block_does_not_need_code_bg() {
    let blocks = vec![tool_block(Fill::Code)];
    assert_eq!(full_width_background(&blocks, 5, FULL_BG, CODE_BG), None);
}

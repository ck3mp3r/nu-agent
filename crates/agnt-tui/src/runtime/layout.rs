//! Layout height computation helpers for the TUI runtime frame.

/// Compute the status bar height based on whether left and right content fit
/// within the available inner width.
pub fn compute_status_h(
    available_inner_w: usize,
    left_width: usize,
    right_width: usize,
    has_message: bool,
) -> u16 {
    let base = if right_width == 0 || left_width + right_width <= available_inner_w {
        1
    } else {
        2
    };
    base + u16::from(has_message)
}

/// Compute the bottom box height from its stack, input, and status contents.
///
/// `stack_content` counts the stacked-message rows: the two padding rows
/// bracketing the text plus every queued prompt's wrapped rows.
/// [`crate::runtime::render::bottom_box::stacked_messages_height`] derives it
/// from the same renderer that paints the stack. The stack also contributes
/// its one-row gap (above it, separating it from the transcript) when
/// non-empty; the input box contributes its own two border rows.
pub fn compute_bottom_box_height(
    stack_content: u16,
    input_content: u16,
    status_content: u16,
) -> u16 {
    let input_borders = 2u16;
    let stack_gap = u16::from(stack_content > 0);
    stack_content
        .saturating_add(stack_gap)
        .saturating_add(input_content)
        .saturating_add(input_borders)
        .saturating_add(status_content)
}

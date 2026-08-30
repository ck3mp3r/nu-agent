//! Layout height computation helpers for the TUI runtime frame.

/// Compute the status bar height based on whether left and right content fit
/// within the available inner width.
pub fn compute_status_h(available_inner_w: usize, left_width: usize, right_width: usize) -> u16 {
    if right_width == 0 || left_width + right_width <= available_inner_w {
        1
    } else {
        2
    }
}

/// Compute the bottom box height from its queue, input, and status contents.
pub fn compute_bottom_box_height(
    queue_content: u16,
    input_content: u16,
    status_content: u16,
) -> u16 {
    let borders = 2u16;
    let dividers = 1u16;
    borders + dividers + queue_content + input_content + status_content
}

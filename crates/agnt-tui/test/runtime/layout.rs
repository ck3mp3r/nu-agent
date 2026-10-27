use super::{compute_bottom_box_height, compute_status_h};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_compute_status_h_fits_without_message() -> Result<()> {
    // -- Exec
    let h = compute_status_h(100, 40, 30, false);

    // -- Check
    assert_eq!(h, 1);
    Ok(())
}

#[test]
fn test_compute_status_h_wraps_without_message() -> Result<()> {
    // -- Exec
    let h = compute_status_h(100, 80, 30, false);

    // -- Check
    assert_eq!(h, 2);
    Ok(())
}

#[test]
fn test_compute_status_h_zero_right_width_without_message() -> Result<()> {
    // -- Exec
    let h = compute_status_h(100, 80, 0, false);

    // -- Check
    assert_eq!(h, 1);
    Ok(())
}

#[test]
fn test_compute_status_h_message_adds_row_when_fits() -> Result<()> {
    // -- Exec
    let without = compute_status_h(100, 40, 30, false);
    let with = compute_status_h(100, 40, 30, true);

    // -- Check
    assert_eq!(with, without + 1);
    Ok(())
}

#[test]
fn test_compute_status_h_message_adds_row_when_wraps() -> Result<()> {
    // -- Exec
    let without = compute_status_h(100, 80, 30, false);
    let with = compute_status_h(100, 80, 30, true);

    // -- Check
    assert_eq!(with, without + 1);
    Ok(())
}

/// THE `compute_bottom_box_height` function SHALL return
/// `stack + (stack > 0 ? 1 : 0) + input + 2 + status`.
#[test]
fn test_compute_bottom_box_height_empty_queue() -> Result<()> {
    // -- Exec
    let h = compute_bottom_box_height(0, 1, 2);

    // -- Check: no stack rows, no gap, input borders (2), input (1), status (2).
    assert_eq!(h, 5);
    Ok(())
}

#[test]
fn test_compute_bottom_box_height_one_stack_row_adds_row_and_gap() -> Result<()> {
    // -- Exec
    let h = compute_bottom_box_height(1, 1, 2);

    // -- Check: one stack row, one gap row above it, input borders (2), input
    // (1), status (2).
    assert_eq!(h, 7);
    Ok(())
}

/// WHEN the stack's wrapped text rows and its two padding rows are reserved,
/// THE `compute_bottom_box_height` function SHALL count every stack row plus
/// the gap row above the stack.
#[test]
fn test_compute_bottom_box_height_wrapped_stack_rows_and_padding() -> Result<()> {
    // -- Exec: four stack rows (two padding rows plus two wrapped text rows).
    let h = compute_bottom_box_height(4, 1, 2);

    // -- Check: four stack rows, one gap row, input borders (2), input (1),
    // status (2).
    assert_eq!(h, 10);
    Ok(())
}

/// THE `compute_bottom_box_height` function SHALL add exactly one row per
/// additional stack row once the stack is non-empty.
#[test]
fn test_compute_bottom_box_height_each_extra_stack_row_adds_one() -> Result<()> {
    // -- Exec
    let one = compute_bottom_box_height(1, 1, 2);
    let four = compute_bottom_box_height(4, 1, 2);

    // -- Check
    assert_eq!(four - one, 3);
    Ok(())
}

/// IF the stack content exceeds the terminal height, THEN THE
/// `compute_bottom_box_height` function SHALL saturate instead of overflowing.
#[test]
fn test_compute_bottom_box_height_saturates() -> Result<()> {
    // -- Exec
    let h = compute_bottom_box_height(u16::MAX, 1, 2);

    // -- Check
    assert_eq!(h, u16::MAX);
    Ok(())
}

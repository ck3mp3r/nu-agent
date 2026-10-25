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
/// `queue + (queue > 0 ? 1 : 0) + input + 2 + status`.
#[test]
fn test_compute_bottom_box_height_empty_queue() -> Result<()> {
    // -- Exec
    let h = compute_bottom_box_height(0, 1, 2);

    // -- Check: no stack rows, no gap, input borders (2), input (1), status (2).
    assert_eq!(h, 5);
    Ok(())
}

#[test]
fn test_compute_bottom_box_height_queued_adds_stack_rows_and_gap() -> Result<()> {
    // -- Exec
    let h = compute_bottom_box_height(2, 1, 2);

    // -- Check: two stack rows, one gap row, input borders (2), input (1), status (2).
    assert_eq!(h, 8);
    Ok(())
}

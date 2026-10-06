use super::compute_status_h;

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

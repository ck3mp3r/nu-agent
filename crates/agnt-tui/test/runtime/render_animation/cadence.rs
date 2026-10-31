use super::*;

/// The spinner cadence in milliseconds, read from the status module so the test
/// names no literal of its own.
const CADENCE: u128 = crate::runtime::SPINNER_CADENCE_MS;

/// WHEN the loop is idle, THE spinner-cadence gate SHALL report no advance at
/// every clock reading, so a timer tick alone never renders a frame.
#[test]
fn spinner_cadence_idle_reports_no_advance() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check
    for step in 0..8u128 {
        assert!(
            !coord.spinner_cadence_advanced(step * CADENCE),
            "an idle loop must never report a cadence advance (step {step})"
        );
    }
    Ok(())
}

/// WHILE the loop animates, WHEN the render loop's timer ticks, THE
/// spinner-cadence gate SHALL report an advance on the first tick and once per
/// cadence step thereafter, and SHALL report no advance twice at the same step.
#[test]
fn spinner_cadence_advances_once_per_step() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.phase = UiPhase::Busy;

    // -- Exec & Check
    assert!(
        coord.spinner_cadence_advanced(0),
        "the first animated tick must render"
    );
    assert!(
        !coord.spinner_cadence_advanced(CADENCE - 1),
        "a repeated step must not render"
    );
    assert!(
        coord.spinner_cadence_advanced(CADENCE),
        "the next step must render"
    );
    assert!(
        !coord.spinner_cadence_advanced(CADENCE),
        "the same step must stay silent"
    );
    assert!(
        coord.spinner_cadence_advanced(3 * CADENCE),
        "a skipped step must still render once"
    );
    Ok(())
}

/// WHEN the animation stops, THE spinner-cadence gate SHALL forget the last
/// step, so the next animation renders its first frame at once.
#[test]
fn spinner_cadence_resets_when_animation_stops() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.phase = UiPhase::Busy;
    assert!(coord.spinner_cadence_advanced(2 * CADENCE));

    // -- Exec: the turn completes, so nothing animates at the same clock
    // reading.
    coord.state.phase = UiPhase::Idle;
    let advanced_while_stopped = coord.spinner_cadence_advanced(2 * CADENCE);

    // -- Check
    assert!(
        !advanced_while_stopped,
        "a stopped animation must not render"
    );
    coord.state.phase = UiPhase::Busy;
    assert!(
        coord.spinner_cadence_advanced(2 * CADENCE),
        "the next animation's first step must render at once"
    );
    Ok(())
}

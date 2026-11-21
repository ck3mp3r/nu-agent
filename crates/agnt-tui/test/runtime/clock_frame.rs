//! The clock-only paint path: a spinner step repaints the cells that read the
//! frame clock, and nothing else.
//!
//! The painted output must be byte-identical to a full recomposition at the same
//! clock reading — only the work differs. So every test here compares the two
//! frames cell by cell, not just the glyph.

use super::*;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// The surface the frames paint onto.
const COLUMNS: u16 = 80;
const ROWS: u16 = 24;

/// The clock reading the fixture's first frame paints at.
const FIRST_TICK: u128 = 100_000;

/// A coordinator holding one animated surface: a busy turn, a streamed answer,
/// and an in-progress call whose indicator cycles with the frame clock.
fn animated_coordinator() -> RuntimeCoordinator {
    let mut coordinator = RuntimeCoordinator::new(COLUMNS, ROWS, None);
    coordinator.state.phase = UiPhase::Busy;
    push_message_line(
        &mut coordinator.state,
        MessageRole::Assistant,
        "streaming answer",
    );
    push_entry(&mut coordinator, Some(ItemStatus::InProgress));
    coordinator
}

/// Render one full frame of `coordinator` at `now_millis` onto `terminal`.
fn render_full(
    coordinator: &mut RuntimeCoordinator,
    terminal: &mut Terminal<TestBackend>,
    now_millis: u128,
) -> Result<()> {
    coordinator.set_frame_clock_millis(now_millis);
    coordinator.render_frame(&mut Some(terminal))?;
    Ok(())
}

/// WHEN a tick advances only the spinner glyphs, THEN THE painted output SHALL
/// be byte-identical to a full recomposition at the same clock reading.
#[test]
fn test_runtime_clock_frame_matches_a_full_recomposition() -> Result<()> {
    // -- Setup & Fixtures: two identical surfaces, painted at the same reading.
    let mut clock_painted = animated_coordinator();
    let mut recomposed = animated_coordinator();
    let mut clock_terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS))?;
    let mut full_terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS))?;
    render_full(&mut clock_painted, &mut clock_terminal, FIRST_TICK)?;
    render_full(&mut recomposed, &mut full_terminal, FIRST_TICK)?;
    let first_frame = full_terminal.backend().buffer().clone();

    // -- Exec: the next spinner step repaints the clock cells alone; the same
    // step recomposes the whole frame on the other surface.
    let next_tick = FIRST_TICK + crate::runtime::SPINNER_CADENCE_MS;
    clock_painted.set_frame_clock_millis(next_tick);
    clock_painted.render_clock_frame(&mut Some(&mut clock_terminal))?;
    render_full(&mut recomposed, &mut full_terminal, next_tick)?;

    // -- Check: the step moved the frame, and both surfaces show it.
    let moved = cells_changed(&first_frame, full_terminal.backend().buffer());
    assert!(
        moved >= 2,
        "the step must move the status spinner and the running call's indicator; {moved} cell(s) moved"
    );
    assert_eq!(
        clock_terminal.backend().buffer(),
        full_terminal.backend().buffer(),
        "a clock-only frame must paint exactly what a full recomposition paints"
    );
    Ok(())
}

/// The number of cells whose content differs between two frames.
fn cells_changed(before: &ratatui::buffer::Buffer, after: &ratatui::buffer::Buffer) -> usize {
    before
        .content
        .iter()
        .zip(&after.content)
        .filter(|(before, after)| before != after)
        .count()
}

/// WHEN a tick advances only the spinner glyphs, THEN THE full-frame
/// recomposition counter SHALL stay where it was and the clock-repaint counter
/// SHALL advance by one.
#[test]
fn test_runtime_clock_frame_does_not_recompose_the_frame() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = animated_coordinator();
    let mut terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS))?;
    render_full(&mut coordinator, &mut terminal, FIRST_TICK)?;

    // -- Exec
    coordinator.set_frame_clock_millis(FIRST_TICK + crate::runtime::SPINNER_CADENCE_MS);
    coordinator.render_clock_frame(&mut Some(&mut terminal))?;

    // -- Check
    let timing = coordinator.frame_timing();
    assert_eq!(
        timing.recomposed_frames(),
        1,
        "only the fixture frame composes"
    );
    assert_eq!(
        timing.clock_frames(),
        1,
        "the spinner step repaints the clock cells"
    );
    assert_eq!(timing.frames_painted(), 2);
    Ok(())
}

/// WHEN a clock-only tick arrives before any frame has painted, THEN THE
/// coordinator SHALL compose the whole frame: there is no painted frame to
/// patch.
#[test]
fn test_runtime_clock_frame_without_a_painted_frame_recomposes() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = animated_coordinator();
    let mut terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS))?;
    coordinator.mark_clock_tick();

    // -- Exec
    coordinator.render_if_needed(&mut Some(&mut terminal))?;

    // -- Check
    let timing = coordinator.frame_timing();
    assert_eq!(timing.recomposed_frames(), 1);
    assert_eq!(timing.clock_frames(), 0);
    Ok(())
}

/// WHEN the render-stats row is visible, THEN a clock-only tick SHALL raise a
/// full frame: the row carries the loop's per-frame counters, so a repaint that
/// skips it would leave stale numbers on the surface.
#[test]
fn test_runtime_clock_frame_recomposes_while_the_stats_row_is_visible() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = animated_coordinator();
    coordinator.state.render_stats_visible = true;
    let mut terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS))?;
    render_full(&mut coordinator, &mut terminal, FIRST_TICK)?;

    // -- Exec
    coordinator.mark_clock_tick();
    coordinator.set_last_render_at(Instant::now() - Duration::from_millis(100));
    coordinator.set_frame_clock_millis(FIRST_TICK + crate::runtime::SPINNER_CADENCE_MS);
    coordinator.render_if_needed(&mut Some(&mut terminal))?;

    // -- Check
    let timing = coordinator.frame_timing();
    assert_eq!(
        timing.recomposed_frames(),
        2,
        "a visible stats row must be composed again on every tick"
    );
    assert_eq!(timing.clock_frames(), 0);
    Ok(())
}

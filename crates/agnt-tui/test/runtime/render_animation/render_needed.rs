use super::*;

/// `render_needed` is set by `drain_transport`; `render_if_needed` skips when
/// not dirty or too soon, and fires (clearing the flag) when dirty and elapsed.
#[test]
fn render_needed_gating() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(false);
    assert!(!coord.render_needed());

    // -- Exec
    coord.enqueue_ui_event(UiEvent::Tick);
    coord.drain_transport();

    // -- Check: drain_transport marks the frame dirty.
    assert!(coord.render_needed());

    // -- Setup & Fixtures: not dirty.
    let mut clean = RuntimeCoordinator::new(80, 24, None);
    clean.set_render_needed(false);
    clean.set_last_render_at(Instant::now() - Duration::from_secs(1));
    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;

    // -- Exec & Check: skips when not dirty.
    assert!(clean.render_if_needed(&mut live).is_ok());
    assert!(!clean.render_needed());

    // -- Setup & Fixtures: dirty but too soon.
    let mut soon = RuntimeCoordinator::new(80, 24, None);
    soon.set_render_needed(true);
    soon.set_last_render_at(Instant::now());
    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;

    // -- Exec & Check: skips when the frame interval has not elapsed.
    assert!(soon.render_if_needed(&mut live).is_ok());
    assert!(soon.render_needed());

    // -- Setup & Fixtures: dirty and elapsed.
    let mut elapsed = RuntimeCoordinator::new(80, 24, None);
    elapsed.set_render_needed(true);
    elapsed.set_last_render_at(Instant::now() - Duration::from_secs(1));
    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;

    // -- Exec & Check: fires and clears the flag.
    assert!(elapsed.render_if_needed(&mut live).is_ok());
    assert!(!elapsed.render_needed());
    Ok(())
}

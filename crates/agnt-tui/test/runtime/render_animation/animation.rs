use super::*;

/// `has_active_animation` is true while busy, abort-pending, compacting, or
/// holding an in-progress entry, and false when idle or holding only finished
/// entries.
#[test]
fn has_active_animation_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let mut busy = RuntimeCoordinator::new(80, 24, None);
    busy.state.phase = UiPhase::Busy;

    let mut aborting = RuntimeCoordinator::new(80, 24, None);
    aborting.state.phase = UiPhase::AbortPending;

    let mut compacting = RuntimeCoordinator::new(80, 24, None);
    compacting
        .state
        .compaction
        .start_block(&mut compacting.state.transcript, "test");

    let mut in_progress = RuntimeCoordinator::new(80, 24, None);
    push_entry(&mut in_progress, Some(ItemStatus::InProgress));

    let mut finished = RuntimeCoordinator::new(80, 24, None);
    push_entry(&mut finished, Some(ItemStatus::Done));
    push_entry(&mut finished, Some(ItemStatus::Failed));
    push_entry(&mut finished, Some(ItemStatus::Queued));

    let idle = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check
    let cases: [(&str, &RuntimeCoordinator, bool); 6] = [
        ("busy", &busy, true),
        ("abort pending", &aborting, true),
        ("compaction in progress", &compacting, true),
        ("in-progress entry", &in_progress, true),
        ("finished entries only", &finished, false),
        ("fully idle", &idle, false),
    ];
    for (label, coord, expected) in cases {
        assert_eq!(coord.has_active_animation(), expected, "{label}");
    }
    Ok(())
}

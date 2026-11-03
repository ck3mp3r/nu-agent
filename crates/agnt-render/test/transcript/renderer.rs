use super::renderer::ItemStatus;

// ── ItemStatus ───────────────────────────────────────────────────────────────

/// Terminal statuses render fixed glyphs; `InProgress` cycles one spinner
/// frame every 100 ms and wraps after the full cycle.
#[test]
fn item_status_table() {
    // -- Exec & Check: equality distinguishes terminal statuses.
    assert_eq!(ItemStatus::Done, ItemStatus::Done);
    assert_ne!(ItemStatus::Done, ItemStatus::Failed);

    // -- Check: terminal statuses render fixed glyphs.
    assert_eq!(ItemStatus::Done.indicator_char(0), "✓");
    assert_eq!(ItemStatus::Failed.indicator_char(0), "✕");
    assert_eq!(ItemStatus::Cancelled.indicator_char(0), "⊘");
    assert_eq!(ItemStatus::Queued.indicator_char(0), "•");
    assert_eq!(ItemStatus::Unknown.indicator_char(0), "?");

    // -- Check: InProgress cycles one frame every 100 ms and wraps.
    assert_eq!(ItemStatus::InProgress.indicator_char(0), "⠋");
    assert_eq!(ItemStatus::InProgress.indicator_char(100), "⠙");
    assert_eq!(ItemStatus::InProgress.indicator_char(900), "⠏");
    assert_eq!(ItemStatus::InProgress.indicator_char(1000), "⠋");
}

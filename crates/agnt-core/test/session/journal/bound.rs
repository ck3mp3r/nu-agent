use super::*;

/// The open-store map is bounded: touching more sessions than the bound keeps
/// only the newest `OPEN_STORES` open.
///
/// On the in-memory backend `session_ids()` is exactly the open stores' keys —
/// nothing persists across opens — so the count is the map's size. Without the
/// bound it would be 40.
#[test]
fn rig_store_evicts_the_oldest_past_the_bound() -> Result<()> {
    // -- Setup & Fixtures
    let memory = test_memory();

    // -- Exec: touch more sessions than the bound.
    for i in 0..40 {
        memory
            .rig_store(&format!("conv-{i}"))
            .map_err(|e| format!("rig store should open: {e:?}"))?;
    }

    // -- Check: the map holds the bound, not every session touched.
    let ids = memory
        .session_ids()
        .map_err(|e| format!("session ids should list: {e:?}"))?;
    assert_eq!(
        ids.len(),
        32,
        "the open-store map must stay within its bound; got: {ids:?}"
    );
    assert!(
        ids.contains(&"conv-39".to_string()),
        "the newest session stays open; got: {ids:?}"
    );
    assert!(
        !ids.contains(&"conv-0".to_string()),
        "the oldest session is evicted; got: {ids:?}"
    );
    Ok(())
}

//! The shared model slot's three states: built, pending, building.
//!
//! Two consumers read one slot — the turn executor and the compactor — so a
//! reader that cannot be served must yield instead of waiting: a wedged build
//! then holds only the consumer that started it.

use rig::test_utils::MockCompletionModel;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A slot holding a built mock model.
fn slot() -> ModelSlot {
    ModelSlot::from_model(MockCompletionModel::default().erase())
}

// ---------------------------------------------------------------------------
// Built
// ---------------------------------------------------------------------------

/// WHEN a switch left nothing pending, THE ModelSlot SHALL serve the model it
/// holds from `try_built`.
#[tokio::test]
async fn try_built_serves_a_fresh_slot() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();

    // -- Exec
    let served = slot.try_built();

    // -- Check
    assert!(served.is_some(), "a fresh slot answers its reader");
    assert!(!slot.is_pending(), "nothing marked the slot stale");
    assert!(!slot.is_building(), "nobody took a build");
    Ok(())
}

// ---------------------------------------------------------------------------
// Pending
// ---------------------------------------------------------------------------

/// WHEN a switch marked the slot stale, THE ModelSlot SHALL answer `None` from
/// `try_built`, so the reader yields rather than waits.
#[tokio::test]
async fn try_built_yields_on_a_pending_slot() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();

    // -- Exec
    slot.mark_pending();

    // -- Check
    assert!(
        slot.try_built().is_none(),
        "a stale slot asks its reader to yield"
    );
    assert!(slot.is_pending(), "the staleness stays until a build lands");
    Ok(())
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// WHEN a consumer owns the build, THE ModelSlot SHALL answer `None` from
/// `try_built`, so a second consumer never waits on another's build.
#[tokio::test]
async fn try_built_yields_on_a_building_slot() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();

    // -- Exec
    slot.mark_building();

    // -- Check
    assert!(
        slot.try_built().is_none(),
        "a build in flight is not a reader's to wait on"
    );
    assert!(slot.is_building(), "the ownership stands until an install");
    assert!(!slot.is_pending(), "ownership is not a config staleness");
    Ok(())
}

/// WHEN an install lands, THE ModelSlot SHALL release the build ownership and
/// serve the installed model.
#[tokio::test]
async fn install_releases_the_build_ownership() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();
    slot.mark_pending();
    slot.mark_building();

    // -- Exec
    slot.install(MockCompletionModel::default().erase());

    // -- Check
    assert!(!slot.is_building(), "the install ends the ownership");
    assert!(!slot.is_pending(), "the install answers the staleness");
    assert!(
        slot.try_built().is_some(),
        "the slot is built and fresh after the install"
    );
    Ok(())
}

/// WHILE a build is running, THE ModelSlot SHALL own it; WHEN the build fails,
/// THE ModelSlot SHALL release the ownership and stay stale, so the next use
/// tries again.
#[tokio::test]
async fn ensure_owns_the_build_and_releases_it_on_failure() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();
    slot.mark_pending();
    let reader = slot.clone();

    // -- Exec: a build that fails while a second consumer reads the slot.
    let failing = slot
        .ensure(|| {
            let reader = reader.clone();
            async move {
                assert!(
                    reader.try_built().is_none(),
                    "a reader mid-build yields instead of waiting"
                );
                Err(nu_protocol::LabeledError::new("provider unreachable"))
            }
        })
        .await;

    // -- Check
    assert!(failing.is_err(), "the build's failure reaches the caller");
    assert!(!slot.is_building(), "a failed build releases the ownership");
    assert!(slot.is_pending(), "a failed build leaves the slot stale");
    Ok(())
}

/// WHEN a build lands, THE ModelSlot SHALL install it and clear both the
/// staleness and the ownership.
#[tokio::test]
async fn ensure_installs_the_built_model() -> Result<()> {
    // -- Setup & Fixtures
    let slot = slot();
    slot.mark_pending();

    // -- Exec
    slot.ensure(|| async { Ok(MockCompletionModel::default().erase()) })
        .await?;

    // -- Check
    assert!(!slot.is_building(), "the install ends the ownership");
    assert!(!slot.is_pending(), "the install answers the staleness");
    assert!(
        slot.try_built().is_some(),
        "the slot serves the built model"
    );
    Ok(())
}

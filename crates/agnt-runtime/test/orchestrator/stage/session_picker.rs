use std::sync::Arc;

use super::*;

// ---------------------------------------------------------------------------
// Session picker refresh
// ---------------------------------------------------------------------------

/// WHILE a turn is in flight, WHEN the session picker refreshes, THE stage SHALL
/// answer the hand the surface supplied AND send no worker command.
///
/// The refresh used to travel the worker channel, so the answer stood behind the
/// running turn and the picker stayed empty until it ended.
#[tokio::test]
async fn session_picker_refresh_answers_the_hand_while_worker_busy() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    p.state.worker_active = true;
    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ctx = make_ctx(p.worker_tx, p.blocking_tx, p.concurrent_tx, p.bus, p.state);

    p.stage
        .handle_incoming(
            UiRequest::RefreshSessionPicker {
                reply: Some(reply_tx),
            },
            &mut ctx,
        )
        .await;

    // The catalog is surface state: it answers on the hand the surface supplied,
    // never as a broadcast and never through the worker.
    let catalog = reply_rx
        .try_recv()
        .map_err(|_| "the picker must answer on its reply hand")?;
    assert!(
        catalog.is_ok(),
        "an empty store lists no session, and that is not a failure: {catalog:?}"
    );
    assert!(
        recv_command(p.worker_rx).is_none(),
        "a picker refresh must send no worker command"
    );
    assert!(
        !p.stage.has_pending(),
        "a served refresh must leave nothing pending"
    );
    Ok(())
}

/// WHEN the session store refuses the listing, THE stage SHALL warn with the
/// reader's sentence AND answer the hand with the same failure.
#[tokio::test]
async fn session_picker_refresh_error_warns_and_answers_the_hand() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let mut h = Harness::new();
    let p = h.parts();
    p.state.catalog = unreadable_catalog(&temp_dir)?;
    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ctx = make_ctx(p.worker_tx, p.blocking_tx, p.concurrent_tx, p.bus, p.state);

    // -- Exec
    p.stage
        .handle_incoming(
            UiRequest::RefreshSessionPicker {
                reply: Some(reply_tx),
            },
            &mut ctx,
        )
        .await;

    // -- Check: one sentence, carried whole on the hand and in the footer; no
    // picker options were sent, so the body keeps its empty state.
    let message = reply_rx
        .try_recv()
        .map_err(|_| "the picker must answer on its reply hand")?
        .err()
        .ok_or("the unreadable store must fail the listing")?;
    assert!(
        message.starts_with("Failed to list sessions: "),
        "the reader's sentence travels whole: {message}"
    );
    let warnings = take_warnings(p.narration_rx);
    assert_eq!(warnings, vec![message], "the footer carries the same line");
    assert!(
        !p.stage.has_pending(),
        "a failed refresh must leave nothing pending"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// A reader whose store cannot be opened: the store path names a directory, so
/// the database refuses to open.
fn unreadable_catalog(temp_dir: &tempfile::TempDir) -> Result<PickerCatalog> {
    let root = temp_dir.path().to_path_buf();
    let memory = Arc::new(crate::session::CachedMemory::new(
        crate::session::RigStoreConfig::new(crate::config::StoreType::Sqlite)
            .with_store_path(root.join("a-directory")),
    ));
    std::fs::create_dir_all(root.join("a-directory"))?;
    Ok(PickerCatalog::new(
        root.clone(),
        root.join("agents"),
        crate::config::AgentsConfig::default(),
        memory,
        None,
    ))
}

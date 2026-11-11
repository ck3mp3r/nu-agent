use std::sync::Arc;

use super::*;

// ---------------------------------------------------------------------------
// Agent picker refresh
// ---------------------------------------------------------------------------

/// WHILE a turn is in flight, WHEN the agent picker refreshes, THE stage SHALL
/// answer the hand the surface supplied AND send no worker command.
///
/// The refresh used to travel the worker channel, so the answer stood behind the
/// running turn and the picker stayed empty until it ended.
#[tokio::test]
async fn agent_picker_refresh_answers_the_hand_while_worker_busy() -> Result<()> {
    let mut h = Harness::new();
    let p = h.parts();
    p.state.worker_active = true;
    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ctx = make_ctx(p.worker_tx, p.blocking_tx, p.concurrent_tx, p.bus, p.state);

    p.stage
        .handle_incoming(
            UiRequest::RefreshAgentPicker {
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
        "a scan of an empty root is not a failure: {catalog:?}"
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

/// WHEN the agent picker refreshes, THE hand SHALL carry a fresh scan of the
/// cwd's persona directory.
#[tokio::test]
async fn agent_picker_refresh_answers_with_the_scanned_catalog() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path().to_path_buf();
    let persona_dir = root.join(".agents");
    std::fs::create_dir_all(&persona_dir)?;
    std::fs::write(
        persona_dir.join("reviewer.md"),
        "---\ndescription: Reviewer\n---\nbody\n",
    )?;
    let catalog = PickerCatalog::new(
        root.clone(),
        root.join("agents"),
        crate::config::AgentsConfig::default(),
        Arc::new(crate::session::CachedMemory::new(
            crate::session::RigStoreConfig::in_memory(),
        )),
        None,
    );

    let mut h = Harness::new();
    let p = h.parts();
    p.state.catalog = catalog;
    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ctx = make_ctx(p.worker_tx, p.blocking_tx, p.concurrent_tx, p.bus, p.state);

    // -- Exec
    p.stage
        .handle_incoming(
            UiRequest::RefreshAgentPicker {
                reply: Some(reply_tx),
            },
            &mut ctx,
        )
        .await;

    // -- Check
    let agents = reply_rx
        .try_recv()
        .map_err(|_| "the picker must answer on its reply hand")?
        .map_err(|e| format!("the scan must succeed: {e}"))?;
    let reviewer = agents
        .iter()
        .find(|agent| agent.name == "reviewer")
        .ok_or("the catalog should carry the persisted persona")?;
    assert_eq!(reviewer.description.as_deref(), Some("Reviewer"));
    Ok(())
}

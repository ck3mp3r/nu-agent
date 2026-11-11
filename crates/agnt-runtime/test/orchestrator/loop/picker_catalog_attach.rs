use std::sync::Arc;
use std::time::Duration;

use super::*;

use crate::catalog::PickerCatalog;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::events::UiRequest;
use crate::orchestrator::external::ExternalTask;
use crate::orchestrator::stages::UiRequestHandler;
use crate::orchestrator::stages::ui_request::UiRequestStage;

// ---------------------------------------------------------------------------
// Picker omission on the A2A attach path
// ---------------------------------------------------------------------------

/// WHEN the spine announces that the runtime attached to a peer's session, THE
/// next session-picker refresh SHALL omit that session AND offer the one it
/// left.
///
/// The attach runs inside the worker, so the catalog it must move lives in the
/// loop. The spine row is the one signal both switch paths already publish, so
/// the loop applies the omission there.
#[tokio::test]
async fn test_loop_session_picker_follows_an_a2a_attach() -> Result<()> {
    // -- Setup & Fixtures: the snapshot attached to one of the two sessions.
    let fixture = AttachFixture::new().await?;
    let before = fixture
        .catalog
        .list_sessions()
        .await
        .map_err(|e| format!("the pre-attach listing should read: {e}"))?;
    assert!(
        before
            .iter()
            .all(|info| info.id != fixture.attached_session),
        "the snapshot omits the session it was built on: {before:?}"
    );
    assert!(
        before.iter().any(|info| info.id == fixture.context_session),
        "and it offers the peer's session: {before:?}"
    );

    let harness = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = harness;
    ctx_state.catalog = fixture.catalog.clone();
    // The attach intake: the loop dispatches the attach command itself.
    let (external_task_tx, external_task_rx) = mpsc::channel::<ExternalTask>(16);

    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ui_request = UiRequestStage::new(0);
    let (mut slash, mut permission, mut session) = ((), (), ());
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    // The arrival the boundary accepted: its context id maps onto the seeded
    // peer session, and the loop hands the attach to the worker.
    external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] do work"))
        .await
        .map_err(|_| "the external task channel should be open")?;

    let key = fixture.context_session.clone();
    let driver = async {
        // The worker takes the attach; the router would attach the session and
        // publish the switch row, so the driver does exactly that here.
        match recv_worker_command(&mut worker_rx, "the attach command").await? {
            WorkerCommand::AttachA2aContext {
                context_id,
                prompt,
                task_id,
            } => {
                assert_eq!(context_id, "ctx-1");
                assert_eq!(prompt, "[A2A] do work");
                assert_eq!(task_id, "task-1");
            }
            _ => return Err("the intake must dispatch the attach".into()),
        }
        bus.lifecycle()
            .publish(crate::bus::LifecycleEvent::SessionSwitched {
                from: None,
                to: key.clone(),
            })
            .map_err(|_| "the lifecycle spine should be open")?;
        // The refresh lands after the switch row, so the loop reads the moved
        // catalog.
        event_tx
            .send(OrchestratorEvent::UiRequest(
                UiRequest::RefreshSessionPicker {
                    reply: Some(reply_tx),
                },
            ))
            .await
            .map_err(|_| "the event channel should be open")?;
        let catalog = recv_answer(&mut reply_rx, "the session catalog").await?;
        // Closing the intake is how the loop exits: no cancel source is
        // configured.
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(catalog)
    };

    // -- Exec
    let (result, catalog) = tokio::join!(
        run_orchestrator_loop(
            event_rx,
            SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: task_cancel_rx.take(),
                external_task_rx: Some(external_task_rx),
                external_completion_rx: None,
                external_task_store: None::<&Arc<crate::orchestrator::external::NoExternalTaskStore>>,
            },
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
        driver
    );

    // -- Check: the omission moved with the attach, and the refresh cost the
    // worker nothing.
    result.map_err(|e| format!("the loop should exit cleanly: {e}"))?;
    let listed = catalog?;
    let sessions = listed.map_err(|e| format!("the session read should succeed: {e}"))?;
    assert!(
        sessions
            .iter()
            .all(|info| info.id != fixture.context_session),
        "the picker omits the attached session: {sessions:?}"
    );
    assert!(
        sessions
            .iter()
            .any(|info| info.id == fixture.attached_session),
        "and it offers the session the runtime left: {sessions:?}"
    );
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a picker refresh must send no worker command"
    );
    assert!(
        !ui_request.has_pending(),
        "a served refresh must leave nothing pending"
    );
    Ok(())
}

/// WHEN an attach fails instead of switching, THE refresh SHALL keep omitting
/// the session the runtime is still in.
///
/// The seam matches `SessionSwitched` only, so a failure row moves nothing.
#[tokio::test]
async fn test_loop_session_picker_keeps_its_omission_after_a_failed_attach() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = AttachFixture::new().await?;

    let harness = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = harness;
    ctx_state.catalog = fixture.catalog.clone();
    let (external_task_tx, external_task_rx) = mpsc::channel::<ExternalTask>(16);

    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ui_request = UiRequestStage::new(0);
    let (mut slash, mut permission, mut session) = ((), (), ());
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] do work"))
        .await
        .map_err(|_| "the external task channel should be open")?;

    let driver = async {
        match recv_worker_command(&mut worker_rx, "the attach command").await? {
            WorkerCommand::AttachA2aContext { .. } => {}
            _ => return Err("the intake must dispatch the attach".into()),
        }
        // The attach failed: the router publishes the failure on the spine and
        // no switch row, so the runtime never left the session it is in.
        bus.lifecycle()
            .publish(crate::bus::LifecycleEvent::TaskFailed {
                task_id: "task-1".to_string(),
                error: "the store refused the key".to_string(),
            })
            .map_err(|_| "the lifecycle spine should be open")?;
        event_tx
            .send(OrchestratorEvent::UiRequest(
                UiRequest::RefreshSessionPicker {
                    reply: Some(reply_tx),
                },
            ))
            .await
            .map_err(|_| "the event channel should be open")?;
        let catalog = recv_answer(&mut reply_rx, "the session catalog").await?;
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(catalog)
    };

    // -- Exec
    let (result, catalog) = tokio::join!(
        run_orchestrator_loop(
            event_rx,
            SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: task_cancel_rx.take(),
                external_task_rx: Some(external_task_rx),
                external_completion_rx: None,
                external_task_store: None::<&Arc<crate::orchestrator::external::NoExternalTaskStore>>,
            },
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
        driver
    );

    // -- Check: a failure row moves no omission.
    result.map_err(|e| format!("the loop should exit cleanly: {e}"))?;
    let listed = catalog?;
    let sessions = listed.map_err(|e| format!("the session read should succeed: {e}"))?;
    assert!(
        sessions
            .iter()
            .all(|info| info.id != fixture.attached_session),
        "the picker still omits the session the runtime is in: {sessions:?}"
    );
    assert!(
        sessions
            .iter()
            .any(|info| info.id == fixture.context_session),
        "and it still offers the peer's session: {sessions:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// A reader over a tempdir: the session the snapshot is built on and the peer's
/// session, whose key matches the attach mapping for `ctx-1`.
struct AttachFixture {
    catalog: PickerCatalog,
    attached_session: String,
    context_session: String,
    _temp_dir: tempfile::TempDir,
}

impl AttachFixture {
    async fn new() -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path().to_path_buf();
        let memory = Arc::new(crate::session::CachedMemory::new(
            crate::session::RigStoreConfig::in_memory(),
        ));
        let prefix = crate::session::prefix::dir_prefix(&root);
        let attached_session = format!("{prefix}-attached");
        // The attach derives `{prefix}-{context_id}`: this session IS the key
        // the peer's `ctx-1` maps onto.
        let context_session = crate::orchestrator::external::external_session_key(&prefix, "ctx-1");
        for (id, text) in [
            (&attached_session, "attached"),
            (&context_session, "context"),
        ] {
            seed_session(&memory, id, text).await?;
        }
        let catalog = PickerCatalog::new(
            root.clone(),
            root.join("agents"),
            crate::config::AgentsConfig::default(),
            memory,
            Some(attached_session.clone()),
        );
        Ok(Self {
            catalog,
            attached_session,
            context_session,
            _temp_dir: temp_dir,
        })
    }
}

/// Seed one session with a single message, so the listing counts it.
async fn seed_session(
    memory: &Arc<crate::session::CachedMemory>,
    id: &str,
    text: &str,
) -> Result<()> {
    let store = memory
        .rig_store(id)
        .await
        .map_err(|e| format!("the rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, id)?;
    conversation.extend([crate::types::Message::user(text)]);
    rig_memory::save(&*store, id, &mut conversation)?;
    crate::session::rig_state::write_state_records(
        Arc::clone(&store),
        "test/model".to_string(),
        None,
    )
    .await?;
    Ok(())
}

/// The catalog an answer hand carries, or a failure naming the hand that stayed
/// empty.
async fn recv_answer<T>(rx: &mut mpsc::Receiver<T>, what: &str) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| format!("{what} should be answered"))?
        .ok_or_else(|| format!("{what} hand should stay open").into())
}

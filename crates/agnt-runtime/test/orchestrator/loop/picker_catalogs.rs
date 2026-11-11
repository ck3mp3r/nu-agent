use std::sync::Arc;
use std::time::Duration;

use super::*;

use crate::catalog::PickerCatalog;
use crate::orchestrator::events::UiRequest;
use crate::orchestrator::stages::UiRequestHandler;
use crate::orchestrator::stages::ui_request::UiRequestStage;

// ---------------------------------------------------------------------------
// Picker refreshes while a turn runs
// ---------------------------------------------------------------------------

/// WHILE a turn is in flight, WHEN the surface asks for both picker catalogs,
/// THE loop SHALL answer each reply hand from the catalog reader AND dispatch no
/// worker command.
///
/// The worker serves its command channel in order, so a refresh that travelled
/// the channel would wait for the turn: the pickers would stay empty until it
/// ended. The reply hands are the evidence — each carries its catalog while
/// `worker_active` is true — and the empty worker channel is the proof that no
/// command was sent.
#[tokio::test]
async fn test_loop_picker_refreshes_serve_while_worker_busy() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = PickerFixture::new().await?;
    let harness = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
        ..
    } = harness;
    // The state the defect is about: a turn is running, so every command the
    // loop sends the worker stands behind it.
    ctx_state.worker_active = true;
    ctx_state.catalog = fixture.catalog.clone();

    let (session_reply_tx, mut session_reply_rx) = mpsc::channel(4);
    let (agent_reply_tx, mut agent_reply_rx) = mpsc::channel(4);
    event_tx
        .send(OrchestratorEvent::UiRequest(
            UiRequest::RefreshSessionPicker {
                reply: Some(session_reply_tx),
            },
        ))
        .await
        .map_err(|_| "the event channel should be open")?;
    event_tx
        .send(OrchestratorEvent::UiRequest(
            UiRequest::RefreshAgentPicker {
                reply: Some(agent_reply_tx),
            },
        ))
        .await
        .map_err(|_| "the event channel should be open")?;
    // Closing the intake is how the loop exits: both requests are queued, so
    // they are served before the close is observed.
    drop(event_tx);

    let mut ui_request = UiRequestStage::new(0);
    let (mut slash, mut permission, mut session) = ((), (), ());
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    // -- Exec
    let (result, answers) = tokio::join!(
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut control_rx,
                &mut lifecycle_rx,
                &mut task_cancel_rx,
            ),
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
        async {
            let sessions = recv_answer(&mut session_reply_rx, "the session catalog").await?;
            let agents = recv_answer(&mut agent_reply_rx, "the agent catalog").await?;
            Ok::<_, Box<dyn std::error::Error>>((sessions, agents))
        }
    );

    // -- Check
    result.map_err(|e| format!("the loop should exit cleanly: {e}"))?;
    let (session_catalog, agent_catalog) = answers?;

    let sessions = session_catalog.map_err(|e| format!("the session read should succeed: {e}"))?;
    assert!(
        sessions.iter().any(|info| info.id == fixture.other_session),
        "the session catalog should carry the session the snapshot was not built on: {sessions:?}"
    );
    let agents = agent_catalog.map_err(|e| format!("the agent scan should succeed: {e}"))?;
    assert!(
        agents.iter().any(|agent| agent.name == "fixture-agent"),
        "the agent catalog should carry the fixture persona: {agents:?}"
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

// ---------------------------------------------------------------------------
// Picker omission after a mid-loop switch
// ---------------------------------------------------------------------------

/// WHEN the loop confirms a switch from one session to another, THE next refresh
/// SHALL omit the session switched to AND offer the one left.
///
/// The catalog snapshot is built when the loop starts. Without moving its
/// omission with the runtime, the picker hides the session the user just left
/// and offers the one they are already in — until the loop restarts.
#[tokio::test]
async fn test_loop_session_picker_follows_a_session_switch() -> Result<()> {
    // -- Setup & Fixtures: the snapshot attached to one of the two sessions.
    let fixture = PickerFixture::new().await?;
    let before = fixture
        .catalog
        .list_sessions()
        .await
        .map_err(|e| format!("the pre-switch listing should read: {e}"))?;
    assert!(
        before
            .iter()
            .all(|info| info.id != fixture.attached_session),
        "the snapshot omits the session it was built on: {before:?}"
    );
    assert!(
        before.iter().any(|info| info.id == fixture.other_session),
        "and it offers the other one: {before:?}"
    );

    let harness = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
        ..
    } = harness;
    ctx_state.catalog = fixture.catalog.clone();

    let (reply_tx, mut reply_rx) = mpsc::channel(4);
    let mut ui_request = UiRequestStage::new(0);
    let mut slash = SlashStage;
    let (mut permission, mut session) = ((), ());
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    let switch_id = fixture.other_session.clone();
    // The surface picks the other session: the stage answers it through the
    // worker, and the driver below stands in for that worker.
    event_tx
        .send(OrchestratorEvent::UiRequest(UiRequest::SwitchSession {
            id: switch_id.clone(),
        }))
        .await
        .map_err(|_| "the event channel should be open")?;
    let driver = async {
        // The stage hands the picked session to the worker as a blocking request.
        let cmd = recv_worker_command(&mut worker_rx, "the session switch").await?;
        match cmd {
            WorkerCommand::HandleUiRequest {
                request: UiRequest::SwitchSession { id },
                ..
            } => assert_eq!(
                id, switch_id,
                "the stage must ask for the session the user picked"
            ),
            _ => return Err("the stage must dispatch the picked session".into()),
        }

        // Answer the switch as the router would. The turn command below is the
        // acknowledgement: the loop cannot start a turn before it has applied
        // this response (the prompt defers behind the pending blocking slot and
        // is submitted once the response frees it).
        blocking_tx
            .send(UiRequestResponse::SessionSwitch {
                id: switch_id.clone(),
                result: Ok(Vec::new()),
            })
            .await
            .map_err(|_| "the blocking response hand should be open")?;

        // A prompt takes the worker slot; the turn never finishes here, so the
        // refresh below arrives while one runs.
        event_tx
            .send(OrchestratorEvent::PromptSubmitted {
                text: "first".to_string(),
            })
            .await
            .map_err(|_| "the event channel should be open")?;
        let _turn = recv_worker_command(&mut worker_rx, "the turn").await?;

        event_tx
            .send(OrchestratorEvent::UiRequest(
                UiRequest::RefreshSessionPicker {
                    reply: Some(reply_tx),
                },
            ))
            .await
            .map_err(|_| "the event channel should be open")?;
        let catalog = recv_answer(&mut reply_rx, "the session catalog").await?;
        // Closing the intake is how the loop exits while the turn still runs.
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(catalog)
    };

    // -- Exec
    let (result, catalog) = tokio::join!(
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut control_rx,
                &mut lifecycle_rx,
                &mut task_cancel_rx,
            ),
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

    // -- Check: the omission moved with the switch, and the refresh cost the
    // worker nothing.
    result.map_err(|e| format!("the loop should exit cleanly: {e}"))?;
    let listed = catalog?;
    let sessions = listed.map_err(|e| format!("the session read should succeed: {e}"))?;
    assert!(
        sessions.iter().all(|info| info.id != switch_id),
        "the picker omits the session the user is in now: {sessions:?}"
    );
    assert!(
        sessions
            .iter()
            .any(|info| info.id == fixture.attached_session),
        "and it offers the session the user left: {sessions:?}"
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

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// A reader over a tempdir: two seeded sessions — the one the snapshot is built
/// attached to and the one it offers — and one persona file under the cwd's
/// `.agents` directory.
struct PickerFixture {
    catalog: PickerCatalog,
    /// The session the snapshot was built on: omitted from the listing.
    attached_session: String,
    /// The other session: offered before a switch, and the one a switch moves
    /// the attachment to.
    other_session: String,
    _temp_dir: tempfile::TempDir,
}

impl PickerFixture {
    async fn new() -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path().to_path_buf();
        let memory = Arc::new(crate::session::CachedMemory::new(
            crate::session::RigStoreConfig::in_memory(),
        ));
        let prefix = crate::session::prefix::dir_prefix(&root);
        let attached_session = format!("{prefix}-attached");
        let other_session = format!("{prefix}-other");
        for (id, text) in [(&attached_session, "attached"), (&other_session, "other")] {
            seed_session(&memory, id, text).await?;
        }

        let persona_dir = root.join(".agents");
        std::fs::create_dir_all(&persona_dir)?;
        std::fs::write(
            persona_dir.join("fixture-agent.md"),
            "---\ndescription: Fixture agent\n---\nbody\n",
        )?;

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
            other_session,
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
        .map_err(|_| format!("{what} should be answered while the turn runs"))?
        .ok_or_else(|| format!("{what} hand should stay open").into())
}

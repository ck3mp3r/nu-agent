//! The runtime-owned external task vocabulary, driven through the loop.
//!
//! Every scenario runs `run_orchestrator_loop` over the new external task and
//! completion channels with a mock `ExternalTaskStore`, so the loop arms are
//! exercised the way production reaches them.

use super::*;

use std::sync::Arc;

use crate::conversation::turn::TurnEnd;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::{ExternalCompletion, ExternalTask, external_session_key};
use crate::orchestrator::loop_impl::WORKER_CMD_CAPACITY;

// ---------------------------------------------------------------------------
// External task intake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_external_task_rx_dispatches_turn() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn();
    let task = external_task("task-1", "ctx-1", "[A2A] do work");

    // -- Exec
    rig.external_task_tx
        .send(task)
        .await
        .map_err(|_| "task send failed")?;
    let attach = rig.recv_command("AttachA2aContext (task-1)").await?;

    // -- Check
    match attach {
        WorkerCommand::AttachA2aContext {
            context_id,
            prompt,
            task_id,
        } => {
            assert_eq!(
                context_id, "ctx-1",
                "the attach must carry the task's context id"
            );
            assert_eq!(
                prompt, "[A2A] do work",
                "the attach must carry the task's prompt"
            );
            assert_eq!(
                task_id, "task-1",
                "the attach must carry the task's id so the prompt can be attributed"
            );
        }
        _ => panic!("expected AttachA2aContext"),
    }

    // The router publishes the arrival on the control concern; the loop's own
    // queue admits it and dispatches the turn attributed to it.
    rig.submit("[A2A] do work", "task-1")?;
    let dispatched = rig.recv_command("ExecuteTurn (task-1)").await?;
    match dispatched {
        WorkerCommand::ExecuteTurn { task_id, .. } => {
            assert_eq!(
                task_id.as_deref(),
                Some("task-1"),
                "the turn must be attributed to the external task"
            );
        }
        _ => panic!("expected ExecuteTurn for task-1"),
    }

    // Make the worker idle, then quit so the loop exits cleanly.
    rig.make_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_stacked_external_tasks_keep_their_own_attribution() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn();

    // -- Exec
    // Two tasks arrive back-to-back, before any turn runs: the acceptance
    // window the single pairing slot used to cross.
    rig.external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] one"))
        .await
        .map_err(|_| "first task send failed")?;
    rig.external_task_tx
        .send(external_task("task-2", "ctx-2", "[A2A] two"))
        .await
        .map_err(|_| "second task send failed")?;
    rig.expect_attach("ctx-1", "[A2A] one", "task-1").await?;
    rig.expect_attach("ctx-2", "[A2A] two", "task-2").await?;

    // The loop's own queue dispatches each accepted arrival, one turn at a
    // time, each carrying the id it was admitted under.
    rig.submit("[A2A] one", "task-1")?;
    let first = rig.recv_command("ExecuteTurn (task-1)").await?;
    assert_eq!(
        execute_turn_task_id(first),
        Some("task-1".to_string()),
        "the first stacked task must keep its own attribution"
    );

    rig.make_idle().await?;
    rig.submit("[A2A] two", "task-2")?;
    let second = rig.recv_command("ExecuteTurn (task-2)").await?;
    assert_eq!(
        execute_turn_task_id(second),
        Some("task-2".to_string()),
        "the second stacked task must keep its own attribution"
    );

    // Neither task crossed nor black-holed: nothing was rejected.
    assert!(
        rig.rejections().is_empty(),
        "stacked tasks must not be rejected"
    );
    rig.make_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_external_task_arriving_during_a_turn_is_accepted() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn_busy();

    // -- Exec
    rig.external_task_tx
        .send(external_task("task-2", "ctx-2", "[A2A] second"))
        .await
        .map_err(|_| "task send failed")?;
    rig.expect_attach("ctx-2", "[A2A] second", "task-2").await?;

    // -- Check
    assert!(
        rig.rejections().is_empty(),
        "a task stacked behind a running turn must be accepted, not rejected"
    );

    rig.make_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_external_task_rx_busy_worker_accepts_without_a_store() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn_without_store();

    // -- Exec
    rig.external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] do work"))
        .await
        .map_err(|_| "task send failed")?;

    // -- Check
    // Accepting a task reaches no store: only a rejection does.
    rig.expect_attach("ctx-1", "[A2A] do work", "task-1")
        .await?;
    assert!(
        rig.rejections().is_empty(),
        "a task stacked behind a running turn must not be rejected"
    );

    rig.make_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_external_task_rx_store_error_keeps_loop_running() -> Result<()> {
    // -- Setup & Fixtures
    // A full worker command channel is the remaining rejection path, and this
    // store fails to record the rejection.
    let rig = ExternalRig::spawn_saturated_with_failing_store();

    // -- Exec
    rig.external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] do work"))
        .await
        .map_err(|_| "task send failed")?;
    rig.wait_for_rejection("task-1").await?;

    // -- Check
    // A loop that exited on the store error would drop the event channel, so a
    // successful quit send proves the loop kept running.
    rig.quit().await
}

#[tokio::test]
async fn test_external_task_rx_closed_worker_channel_keeps_loop_running() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn();
    // A closed worker command channel: the receiver is dropped, so `try_send`
    // answers `Closed` instead of `Full`.
    rig.worker_rx = None;

    // -- Exec
    rig.external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] do work"))
        .await
        .map_err(|_| "task send failed")?;

    // -- Check
    // A loop that exited on the closed channel would drop the event channel, so
    // a successful quit send proves the loop kept running.
    rig.quit().await
}

// ---------------------------------------------------------------------------
// External completion intake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_external_completion_dispatches_unattributed_turn() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn();

    // -- Exec
    rig.external_completion_tx
        .send(external_completion("task-9", "remote task-9 completed"))
        .await
        .map_err(|_| "completion send failed")?;
    // A completion reports a REMOTE task. An idle loop dispatches it on the
    // spot — the enqueue helper is the loop's own intake, an internal action,
    // not a command row — so no control event carries it and no surface echo is
    // needed. No local task holds that id, so the turn stays unattributed.
    let dispatched = rig.recv_command("ExecuteTurn (completion)").await?;
    match dispatched {
        WorkerCommand::ExecuteTurn {
            prompt, task_id, ..
        } => {
            assert_eq!(
                prompt, "remote task-9 completed",
                "the completion turn prompt must be the submitted text"
            );
            assert_eq!(
                task_id, None,
                "a completion must not attribute the turn to a local task"
            );
        }
        _ => panic!("expected ExecuteTurn for the completion"),
    }

    rig.make_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_external_completion_rx_buffers_while_worker_busy() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = ExternalRig::spawn_busy();

    // -- Exec
    rig.external_completion_tx
        .send(external_completion("task-9", "remote task-9 completed"))
        .await
        .map_err(|_| "completion send failed")?;

    // -- Check
    // The busy worker must buffer the completion instead of dispatching it.
    rig.expect_no_command("ExecuteTurn (completion)").await?;

    rig.make_idle().await?;
    let dispatched = rig
        .recv_command("ExecuteTurn (buffered completion)")
        .await?;
    match dispatched {
        WorkerCommand::ExecuteTurn {
            prompt, task_id, ..
        } => {
            assert_eq!(
                prompt, "remote task-9 completed",
                "the buffered completion must be dispatched once the worker is idle"
            );
            assert_eq!(
                task_id, None,
                "a drained completion reports a remote task, so its turn stays unattributed"
            );
        }
        _ => panic!("expected ExecuteTurn for the buffered completion"),
    }

    // The drained completion started its own turn, so the worker is active
    // again: make it idle before quitting.
    rig.make_idle().await?;
    rig.quit().await
}

// ---------------------------------------------------------------------------
// Session key
// ---------------------------------------------------------------------------

#[test]
fn test_external_session_key_formats_prefix_and_context() {
    // -- Setup & Fixtures
    let prefix = "7f3a9c";
    let context_id = "ctx-42";

    // -- Exec
    let key = external_session_key(prefix, context_id);

    // -- Check
    assert_eq!(
        key, "7f3a9c-ctx-42",
        "the key format IS the context mapping"
    );
}

// region:    --- Support

const NO_COMMAND_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

/// The task attribution a dispatched `ExecuteTurn` carries, or `None` for any
/// other command.
fn execute_turn_task_id(command: WorkerCommand) -> Option<String> {
    match command {
        WorkerCommand::ExecuteTurn { task_id, .. } => task_id,
        _ => None,
    }
}

/// One running loop plus the channels a test drives it with.
struct ExternalRig {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    /// The control intake: the router publishes an arrival here, and the loop's
    /// own queue admits it.
    control: crate::bus::ControlTx,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    external_task_tx: mpsc::Sender<ExternalTask>,
    external_completion_tx: mpsc::Sender<ExternalCompletion>,
    store: Option<Arc<MockExternalStore>>,
    handle: tokio::task::JoinHandle<core::result::Result<(), LabeledError>>,
}

impl ExternalRig {
    /// An idle worker and a store that accepts every rejection.
    fn spawn() -> Self {
        Self::build(Some(Arc::new(MockExternalStore::default())), false, false)
    }

    /// A busy worker and a store that accepts every rejection.
    fn spawn_busy() -> Self {
        Self::build(Some(Arc::new(MockExternalStore::default())), true, false)
    }

    /// A busy worker with no store configured.
    fn spawn_without_store() -> Self {
        Self::build(None, true, false)
    }

    /// A worker command channel already at capacity, and a store whose
    /// `reject_task` fails.
    fn spawn_saturated_with_failing_store() -> Self {
        Self::build(Some(Arc::new(MockExternalStore::failing())), false, true)
    }

    fn build(store: Option<Arc<MockExternalStore>>, worker_busy: bool, saturate: bool) -> Self {
        let h = Harness::new();
        let Harness {
            event_tx,
            event_rx,
            worker_tx,
            worker_rx,
            worker_result_tx,
            worker_result_rx,
            blocking_tx,
            blocking_response_rx,
            concurrent_tx,
            concurrent_response_rx,
            control_rx: _,
            lifecycle_rx,
            mut task_cancel_rx,
            bus,
            ctx_state,
        } = h;
        let (external_task_tx, external_task_rx) = mpsc::channel::<ExternalTask>(16);
        let (external_completion_tx, external_completion_rx) =
            mpsc::channel::<ExternalCompletion>(16);
        // One hand for the loop sources.
        let loop_control_rx = bus.control().subscribe();
        // The test's own hand on the same concern, standing in for the router.
        let control = bus.control().clone();
        if saturate {
            // Fill the worker command channel so the intake arm reaches its
            // `Full` branch instead of a `try_send` that succeeds.
            for _ in 0..WORKER_CMD_CAPACITY {
                let _ = worker_tx.try_send(WorkerCommand::NewSession);
            }
        }
        let loop_store = store.clone();

        let handle = tokio::spawn(async move {
            let mut worker_result_rx = worker_result_rx;
            let mut blocking_response_rx = blocking_response_rx;
            let mut concurrent_response_rx = concurrent_response_rx;
            let mut control_rx = loop_control_rx;
            let mut lifecycle_rx = lifecycle_rx;
            let mut ctx_state = ctx_state;
            ctx_state.worker_active = worker_busy;

            let mut slash = SlashStage;
            let mut permission = MockPermission::new();
            let mut ui_request = MockUiRequest::new();
            let mut session = MockSession::new();
            let mut ctx = make_ctx(
                &worker_tx,
                &blocking_tx,
                &concurrent_tx,
                &bus,
                &mut ctx_state,
            );
            let store = loop_store.as_ref();
            let sources = SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: task_cancel_rx.take(),
                external_task_rx: Some(external_task_rx),
                external_completion_rx: Some(external_completion_rx),
                external_task_store: store,
            };
            run_orchestrator_loop(
                event_rx,
                sources,
                Stages {
                    slash: &mut slash,
                    permission: &mut permission,
                    ui_request: &mut ui_request,
                    session: &mut session,
                },
                &mut ctx,
            )
            .await
        });

        Self {
            event_tx,
            control,
            worker_rx,
            worker_result_tx,
            external_task_tx,
            external_completion_tx,
            store,
            handle,
        }
    }

    async fn recv_command(&mut self, what: &str) -> Result<WorkerCommand> {
        recv_worker_command(&mut self.worker_rx, what).await
    }

    /// Assert no worker command is dispatched within the quiet budget.
    async fn expect_no_command(&mut self, what: &str) -> Result<()> {
        let rx = self.worker_rx.as_mut().ok_or("worker_rx present")?;
        match tokio::time::timeout(NO_COMMAND_BUDGET, rx.recv()).await {
            Err(_) => Ok(()),
            Ok(_) => Err(format!("{what}: no command should be dispatched").into()),
        }
    }

    /// Receive the next attach command with a timeout and assert it names
    /// `context_id`, `prompt`, and `task_id`.
    async fn expect_attach(&mut self, context_id: &str, prompt: &str, task_id: &str) -> Result<()> {
        match self
            .recv_command(&format!("AttachA2aContext ({context_id})"))
            .await?
        {
            WorkerCommand::AttachA2aContext {
                context_id: actual_context,
                prompt: actual_prompt,
                task_id: actual_task_id,
            } => {
                assert_eq!(actual_context, context_id, "the attach context id");
                assert_eq!(actual_prompt, prompt, "the attach prompt");
                assert_eq!(actual_task_id, task_id, "the attach task id");
                Ok(())
            }
            _ => Err("expected AttachA2aContext".into()),
        }
    }

    fn submit(&self, text: &str, task_id: &str) -> Result<()> {
        self.control
            .publish(crate::bus::ControlCommand::EnqueueExternalPrompt {
                task_id: task_id.to_string(),
                prompt: text.to_string(),
            })
            .map(|_| ())
            .map_err(|_| "the control channel should be open".into())
    }

    async fn make_idle(&self) -> Result<()> {
        self.worker_result_tx
            .send(TurnResult::from_end(TurnEnd::Completed {
                value: Value::nothing(Span::test_data()),
            }))
            .await
            .map_err(|_| "worker result send failed".into())
    }

    fn rejections(&self) -> Vec<(String, String)> {
        match &self.store {
            Some(store) => store.rejections(),
            None => Vec::new(),
        }
    }

    async fn wait_for_rejection(&self, task_id: &str) -> Result<(String, String)> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(rejection) = self
                .rejections()
                .into_iter()
                .find(|(rejected_id, _)| rejected_id == task_id)
            {
                return Ok(rejection);
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("external task {task_id} should be rejected").into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    }

    async fn quit(self) -> Result<()> {
        self.event_tx
            .send(OrchestratorEvent::Quit)
            .await
            .map_err(|_| "quit send failed")?;
        match self.handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(format!("the loop should exit cleanly: {e}").into()),
            Err(e) => Err(format!("the loop task should not panic: {e}").into()),
        }
    }
}

// endregion: --- Support

//! The buffered-completion scenarios: arrival order, the guarded drain, and the
//! flush on graceful quit.
//!
//! Every scenario drives `run_orchestrator_loop` over the real completion
//! channel with the real slash stage, so a dispatched completion is observed on
//! the worker command channel exactly as production sends it.

// region:    --- Modules

#[path = "completions/row.rs"]
mod row;

// endregion: --- Modules

use super::*;

use crate::conversation::turn::TurnEnd;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::{ExternalCompletion, NoExternalTaskStore};

// ---------------------------------------------------------------------------
// Arrival order and the per-cycle priority
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_completion_buffer_dispatches_oldest_first_before_queued_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(true, false);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    rig.send_completion("task-8", "remote task-8 completed")
        .await?;
    // A prompt from outside the surface waits behind the buffered completions.
    rig.enqueue_prompt("task-1", "[A2A] queued work")?;

    // -- Exec
    // One idle cycle per completion, oldest first, each unattributed.
    // A busy worker buffers both arrivals before the turn it runs ends.
    rig.expect_no_turn("a busy worker must buffer the completions")
        .await?;
    rig.become_idle().await?;
    let first = rig
        .recv_execute_turn("the oldest buffered completion")
        .await?;
    rig.become_idle().await?;
    let second = rig
        .recv_execute_turn("the second buffered completion")
        .await?;
    rig.become_idle().await?;
    let queued = rig.recv_execute_turn("the queued external prompt").await?;

    // -- Check
    assert_eq!(
        first,
        ("remote task-9 completed".to_string(), None),
        "the first completion to arrive must dispatch first, unattributed"
    );
    assert_eq!(
        second,
        ("remote task-8 completed".to_string(), None),
        "the second completion to arrive must dispatch second, unattributed"
    );
    assert_eq!(
        queued,
        ("[A2A] queued work".to_string(), Some("task-1".to_string())),
        "the queued external prompt must wait until the buffer is empty"
    );

    rig.become_idle().await?;
    rig.quit().await
}

#[tokio::test]
async fn test_repeated_completion_keeps_one_entry_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(true, false);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    rig.send_completion("task-8", "remote task-8 completed")
        .await?;
    // A repeat for the task already held: it updates that entry, it does not take
    // a second place in the line.
    rig.send_completion("task-9", "remote task-9 completed again")
        .await?;

    // -- Exec
    rig.expect_no_turn("a busy worker must buffer the completions")
        .await?;
    rig.become_idle().await?;
    let first = rig
        .recv_execute_turn("the first buffered completion")
        .await?;
    rig.become_idle().await?;
    let second = rig
        .recv_execute_turn("the second buffered completion")
        .await?;
    rig.become_idle().await?;
    rig.expect_no_turn("a repeated completion must not queue a second turn")
        .await?;

    // -- Check
    assert_eq!(
        first,
        ("remote task-9 completed again".to_string(), None),
        "the repeat must update the entry in its original place"
    );
    assert_eq!(
        second,
        ("remote task-8 completed".to_string(), None),
        "the repeat must not reorder the entry behind the later arrival"
    );

    rig.quit().await
}

// ---------------------------------------------------------------------------
// The guard
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_completion_drain_defers_while_blocking_request_pending() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(false, true);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;

    // -- Exec
    rig.expect_no_turn("a blocking request pending must defer the completion")
        .await?;
    rig.clear_blocking_request().await?;
    let dispatched = rig
        .recv_execute_turn("the completion the blocking request deferred")
        .await?;

    // -- Check
    assert_eq!(
        dispatched,
        ("remote task-9 completed".to_string(), None),
        "the completion must run once the blocking request completes"
    );

    rig.become_idle().await?;
    rig.quit().await
}

// ---------------------------------------------------------------------------
// Graceful quit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_quit_flushes_buffered_completions_before_exit() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(true, false);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    rig.send_completion("task-8", "remote task-8 completed")
        .await?;
    // Both arrivals are buffered before the quit is requested.
    rig.expect_no_turn("a busy worker must buffer the completions")
        .await?;
    rig.send_quit().await?;

    // -- Exec
    rig.become_idle().await?;
    let first = rig.recv_execute_turn("the first flush turn").await?;
    rig.become_idle().await?;
    let second = rig.recv_execute_turn("the second flush turn").await?;
    rig.become_idle().await?;
    rig.finish().await?;

    // -- Check
    assert_eq!(
        first,
        ("remote task-9 completed".to_string(), None),
        "the flush must run the oldest buffered completion first"
    );
    assert_eq!(
        second,
        ("remote task-8 completed".to_string(), None),
        "the flush must run every buffered completion before exiting"
    );
    Ok(())
}

#[tokio::test]
async fn test_completion_arriving_during_flush_window_joins_the_flush() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(true, false);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    // The completion is buffered before the quit is requested, so the quit has
    // something to flush.
    rig.expect_no_turn("a busy worker must buffer the completion")
        .await?;
    rig.send_quit().await?;
    rig.become_idle().await?;
    let first = rig.recv_execute_turn("the first flush turn").await?;

    // -- Exec
    // The quit is pending and its first flush turn is in flight: this arrival
    // must join the flush, not be deferred into a slot no quit path runs.
    rig.send_completion("task-8", "remote task-8 completed")
        .await?;
    // The arrival is buffered while the flush turn runs.
    rig.expect_no_turn("a running flush turn must buffer the arrival")
        .await?;
    rig.become_idle().await?;
    let second = rig
        .recv_execute_turn("the completion that joined the flush")
        .await?;
    rig.become_idle().await?;
    rig.finish().await?;

    // -- Check
    assert_eq!(
        first,
        ("remote task-9 completed".to_string(), None),
        "the buffered completion must own the first flush turn"
    );
    assert_eq!(
        second,
        ("remote task-8 completed".to_string(), None),
        "a completion that arrives during the flush window must also run"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Torn-down source channel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_closed_completion_channel_exits_without_starting_a_turn() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(true, false);
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    rig.send_completion("task-8", "remote task-8 completed")
        .await?;

    // -- Exec
    // The producer is gone: the loop may not start a turn for what it still
    // holds, so it names each abandoned id and exits.
    rig.close_completions();
    rig.expect_no_turn("a torn-down source must start no turn")
        .await?;

    // -- Check
    rig.finish().await
}

// ---------------------------------------------------------------------------
// Idle arrival
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_idle_completion_dispatches_its_turn_immediately() -> Result<()> {
    // -- Setup & Fixtures
    let mut rig = CompletionRig::build(false, false);

    // -- Exec
    rig.send_completion("task-9", "remote task-9 completed")
        .await?;
    let dispatched = rig.recv_execute_turn("the idle completion").await?;

    // -- Check
    assert_eq!(
        dispatched,
        ("remote task-9 completed".to_string(), None),
        "an idle loop must dispatch the completion on the spot, unattributed"
    );

    rig.become_idle().await?;
    rig.quit().await
}

// region:    --- Support

const NO_TURN_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

/// One running loop plus the channels a completion scenario drives it with.
struct CompletionRig {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    /// The control intake, standing in for the router that hands the loop its
    /// external prompts.
    control: crate::bus::ControlTx,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    completion_tx: Option<mpsc::Sender<ExternalCompletion>>,
    handle: tokio::task::JoinHandle<core::result::Result<(), LabeledError>>,
}

impl CompletionRig {
    /// A loop whose worker is busy or idle, with or without a blocking request
    /// already pending. The slash stage is the real one, so a dispatched
    /// completion starts a real turn and flips the worker's active flag.
    fn build(worker_busy: bool, blocking_pending: bool) -> Self {
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
        let (completion_tx, completion_rx) = mpsc::channel::<ExternalCompletion>(16);
        // One hand for the loop sources.
        let loop_control_rx = bus.control().subscribe();
        // The test's own hand on the same concern, standing in for the router.
        let control = bus.control().clone();

        // The loop takes its own hand; the test keeps one to answer a request.
        let rig_blocking_tx = blocking_tx.clone();
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
            ui_request.blocking_pending = blocking_pending;
            ui_request.pending = blocking_pending;
            let mut session = MockSession::new();
            let mut ctx = make_ctx(
                &worker_tx,
                &blocking_tx,
                &concurrent_tx,
                &bus,
                &mut ctx_state,
            );
            let sources = SourceChannels::<NoExternalTaskStore> {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: task_cancel_rx.take(),
                external_task_rx: None,
                external_completion_rx: Some(completion_rx),
                external_task_store: None,
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
            blocking_tx: rig_blocking_tx,
            worker_rx,
            worker_result_tx,
            completion_tx: Some(completion_tx),
            handle,
        }
    }

    async fn send_completion(&self, task_id: &str, prompt: &str) -> Result<()> {
        self.completion_tx
            .as_ref()
            .ok_or("the completion channel should be open")?
            .send(ExternalCompletion {
                task_id: task_id.to_string(),
                prompt: prompt.to_string(),
            })
            .await
            .map_err(|_| "completion send failed".into())
    }

    /// Close the completion channel, as a producer that went away does.
    fn close_completions(&mut self) {
        self.completion_tx = None;
    }

    fn enqueue_prompt(&self, task_id: &str, prompt: &str) -> Result<()> {
        self.control
            .publish(crate::bus::ControlCommand::EnqueueExternalPrompt {
                task_id: task_id.to_string(),
                prompt: prompt.to_string(),
            })
            .map(|_| ())
            .map_err(|_| "the control channel should be open".into())
    }

    /// Answer the outstanding blocking request, which frees the drain.
    async fn clear_blocking_request(&self) -> Result<()> {
        self.blocking_tx
            .send(UiRequestResponse::ModelSwitch(Err(
                "the request needs no answer".to_string(),
            )))
            .await
            .map_err(|_| "blocking response send failed".into())
    }

    async fn send_quit(&self) -> Result<()> {
        self.event_tx
            .send(OrchestratorEvent::Quit)
            .await
            .map_err(|_| "quit send failed".into())
    }

    /// End the turn in flight, as the worker does when it finishes one.
    async fn become_idle(&self) -> Result<()> {
        self.worker_result_tx
            .send(TurnResult::from_end(TurnEnd::Completed {
                value: Value::nothing(Span::test_data()),
            }))
            .await
            .map_err(|_| "worker result send failed".into())
    }

    /// Receive the next turn the loop starts, with a timeout.
    async fn recv_execute_turn(&mut self, what: &str) -> Result<(String, Option<String>)> {
        match recv_worker_command(&mut self.worker_rx, what).await? {
            WorkerCommand::ExecuteTurn {
                prompt, task_id, ..
            } => Ok((prompt, task_id)),
            _ => Err(format!("{what}: expected ExecuteTurn").into()),
        }
    }

    /// Assert no turn is dispatched within the quiet budget.
    async fn expect_no_turn(&mut self, what: &str) -> Result<()> {
        let rx = self.worker_rx.as_mut().ok_or("worker_rx present")?;
        match tokio::time::timeout(NO_TURN_BUDGET, rx.recv()).await {
            Err(_) => Ok(()),
            Ok(Some(_)) => Err(format!("{what}: no turn should be dispatched").into()),
            Ok(None) => Ok(()),
        }
    }

    async fn quit(self) -> Result<()> {
        self.send_quit().await?;
        self.finish().await
    }

    /// Await a loop that exits on its own condition.
    async fn finish(self) -> Result<()> {
        match self.handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(format!("the loop should exit cleanly: {e}").into()),
            Err(e) => Err(format!("the loop task should not panic: {e}").into()),
        }
    }
}

// endregion: --- Support

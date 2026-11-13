//! The prompt row a dispatched completion draws.
//!
//! A completion notification is the prompt of a real conversation turn, and the
//! loop is its accepting boundary: no router sees it. The row the surface draws
//! for that turn is therefore published here, once per dispatched completion.

use super::*;

use agnt_kernel::PromptRecord;
use agnt_kernel::SessionRecord;
use agnt_kernel::bus::Subscription;

use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::{ExternalCompletion, NoExternalTaskStore};

const FIRST_PROMPT: &str =
    "[A2A] Task 7f3a1c by peer-agent: completed. Call tasks_get for details.";
const SECOND_PROMPT: &str = "[A2A] Task 9b2d44 by peer-agent: failed. Call tasks_get for details.";

#[tokio::test]
async fn test_orchestrator_loop_completions_row_a_dispatched_completion_publishes_one_user_row_record()
-> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
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
    } = h;
    // Subscribed before the loop starts, so the row reaches this hand.
    let mut stream_rx = bus.stream().subscribe();
    let (completion_tx, completion_rx) = mpsc::channel::<ExternalCompletion>(16);
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

    // -- Exec: an idle loop dispatches the arrival on the spot.
    let driver = async {
        completion_tx
            .send(external_completion("task-7", FIRST_PROMPT))
            .await
            .map_err(|_| "completion send failed")?;
        match recv_worker_command(&mut worker_rx, "the completion turn").await? {
            WorkerCommand::ExecuteTurn {
                prompt, task_id, ..
            } => {
                assert_eq!(prompt, FIRST_PROMPT);
                assert_eq!(task_id, None);
            }
            _ => return Err("expected ExecuteTurn".into()),
        }
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
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
        ),
        driver
    );
    driven?;
    result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;

    // -- Check: the dispatched turn drew exactly one user row, carrying the
    // completion's own text and no attribution.
    let prompts = prompt_rows(&mut stream_rx);
    assert_eq!(
        prompts.len(),
        1,
        "a dispatched completion draws exactly one row, got {prompts:?}"
    );
    assert_eq!(prompts[0].text, FIRST_PROMPT);
    assert!(
        prompts[0].task_id.is_none(),
        "a completion turn belongs to no local attribution, got {:?}",
        prompts[0].task_id
    );
    Ok(())
}

#[tokio::test]
async fn test_orchestrator_loop_completions_row_record_carries_no_identity_stamps() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
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
    } = h;
    let mut stream_rx = bus.stream().subscribe();
    let (completion_tx, completion_rx) = mpsc::channel::<ExternalCompletion>(16);
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

    // -- Exec
    let before = chrono::Utc::now().timestamp_millis();
    let driver = async {
        completion_tx
            .send(external_completion("task-7", FIRST_PROMPT))
            .await
            .map_err(|_| "completion send failed")?;
        recv_worker_command(&mut worker_rx, "the completion turn").await?;
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
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
        ),
        driver
    );
    driven?;
    result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    let after = chrono::Utc::now().timestamp_millis();

    // -- Check: the loop knows when it dispatched the turn and nothing about
    // the model or the persona, so the row records the instant and no identity.
    let prompts = prompt_rows(&mut stream_rx);
    let prompt = prompts
        .first()
        .ok_or("a dispatched completion should draw a row")?;
    let millis = prompt
        .stamps
        .millis
        .ok_or("the dispatching loop should stamp the instant")?;
    assert!(
        (before..=after).contains(&millis),
        "the stamp should be the dispatch instant, got {millis} outside {before}..={after}"
    );
    assert!(
        prompt.stamps.model.is_none(),
        "the loop reaches no model identity, got {:?}",
        prompt.stamps.model
    );
    assert!(
        prompt.stamps.agent.is_none(),
        "the loop reaches no persona identity, got {:?}",
        prompt.stamps.agent
    );
    Ok(())
}

#[tokio::test]
async fn test_orchestrator_loop_completions_row_one_row_per_dispatched_completion_in_buffer_order()
-> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx,
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
    } = h;
    let mut stream_rx = bus.stream().subscribe();
    let (completion_tx, completion_rx) = mpsc::channel::<ExternalCompletion>(16);
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    // A busy worker buffers both arrivals before either turn runs.
    ctx_state.worker_active = true;
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

    // -- Exec: one turn per idle cycle, in the order the completions arrived.
    let driver = async {
        completion_tx
            .send(external_completion("task-7", FIRST_PROMPT))
            .await
            .map_err(|_| "first completion send failed")?;
        completion_tx
            .send(external_completion("task-8", SECOND_PROMPT))
            .await
            .map_err(|_| "second completion send failed")?;
        // Both arrivals are buffered while the worker is busy.
        worker_result_tx
            .send(turn_completed())
            .await
            .map_err(|_| "worker result send failed")?;
        let first = recv_prompt(&mut worker_rx, "the first buffered completion").await?;
        worker_result_tx
            .send(turn_completed())
            .await
            .map_err(|_| "worker result send failed")?;
        let second = recv_prompt(&mut worker_rx, "the second buffered completion").await?;
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>((first, second))
    };
    let (result, driven) = tokio::join!(
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
        ),
        driver
    );
    let (first, second) = driven?;
    result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;

    // -- Check
    assert_eq!(first, FIRST_PROMPT);
    assert_eq!(second, SECOND_PROMPT);
    let prompts = prompt_rows(&mut stream_rx);
    let texts: Vec<String> = prompts.into_iter().map(|prompt| prompt.text).collect();
    assert_eq!(
        texts,
        vec![FIRST_PROMPT.to_string(), SECOND_PROMPT.to_string()],
        "each dispatched completion draws its own row, in buffer order"
    );
    Ok(())
}

// region:    --- Support

/// The prompt rows published on the stream, in publication order.
fn prompt_rows(stream_rx: &mut Subscription<SessionRecord>) -> Vec<PromptRecord> {
    std::iter::from_fn(|| stream_rx.try_recv().ok())
        .filter_map(|record| match record {
            SessionRecord::Prompt(prompt) => Some(prompt),
            _ => None,
        })
        .collect()
}

/// The prompt of the next turn the loop starts.
async fn recv_prompt(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
    what: &str,
) -> Result<String> {
    match recv_worker_command(worker_rx, what).await? {
        WorkerCommand::ExecuteTurn { prompt, .. } => Ok(prompt),
        _ => Err(format!("{what}: expected ExecuteTurn").into()),
    }
}

/// The result the worker sends when a turn finishes without a stop.
fn turn_completed() -> TurnResult {
    TurnResult::from_end(TurnEnd::Completed {
        value: Value::nothing(Span::test_data()),
    })
}

// endregion: --- Support

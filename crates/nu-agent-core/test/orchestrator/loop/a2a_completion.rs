use super::*;

// ---------------------------------------------------------------------------
// A2A completion intake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a2a_completion_rx_dispatches_turn() -> Result<()> {
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
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let (a2a_completion_tx, a2a_completion_rx) = mpsc::channel::<A2aCompletionEvent>(16);

    let loop_task = tokio::spawn(async move {
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
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        let mut sources = make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        );
        sources.a2a_completion_rx = Some(a2a_completion_rx);
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

    let completion = A2aCompletionEvent {
        task_id: "task-2".to_string(),
        agent_name: "agent-b".to_string(),
        result: "all done".to_string(),
        status: TaskState::Completed,
        context_id: None,
    };
    let completion_prompt = completion.to_prompt();
    a2a_completion_tx.send(completion).await.unwrap();

    // The orchestrator enqueues the prompt on the UI bus; the TUI submits it.
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: completion_prompt.clone(),
        })
        .await
        .unwrap();

    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "ExecuteTurn should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(prompt, completion_prompt);
        }
        _ => panic!("expected ExecuteTurn"),
    }

    // Make the worker idle, then quit so the loop exits cleanly.
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn a2a_completion_rx_fails_while_worker_busy() -> Result<()> {
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
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let (a2a_completion_tx, a2a_completion_rx) = mpsc::channel::<A2aCompletionEvent>(16);

    // The completion's task id refers to a task tracked by the store. Model a
    // Working task so the busy-fail has something to transition.
    let store = std::sync::Arc::new(InMemoryTaskStore::default());
    let task2 = store.create_task(None, None, None);
    store
        .update_status(&task2.id, TaskState::Working, None)
        .map_err(|e| format!("task-2 should reach Working: {e}"))?;
    let task2_id = task2.id.clone();
    ctx_state.task_store = Some(std::sync::Arc::clone(&store));

    let loop_task = tokio::spawn(async move {
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
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        let mut sources = make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        );
        sources.a2a_completion_rx = Some(a2a_completion_rx);
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

    // -- Exec
    // First completion: the orchestrator enqueues the prompt on the UI bus and
    // the TUI submits it, which dispatches the turn.
    let completion1 = completion_event("task-1", "first done");
    let completion1_prompt = completion1.to_prompt();
    a2a_completion_tx
        .send(completion1)
        .await
        .map_err(|_| "completion-1 send failed")?;
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: completion1_prompt.clone(),
        })
        .await
        .map_err(|_| "completion-1 submit failed")?;
    let cmd = recv_worker_command(&mut worker_rx, "ExecuteTurn (completion-1)").await?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(
                prompt, completion1_prompt,
                "completion-1 turn prompt must be the submitted text"
            );
        }
        _ => panic!("expected ExecuteTurn for completion-1"),
    }

    // Second completion arrives while the worker is busy. The orchestrator must
    // fail the task instead of leaving it stuck in Working.
    let completion2 = completion_event(&task2_id, "second done");
    a2a_completion_tx
        .send(completion2)
        .await
        .map_err(|_| "completion-2 send failed")?;
    wait_for_task_state(&store, &task2_id, TaskState::Failed).await?;
    assert!(
        worker_rx
            .as_mut()
            .ok_or("worker_rx present")?
            .try_recv()
            .is_err(),
        "completion-2 must not dispatch while the worker is busy"
    );

    // -- Check
    let failed = store
        .get_task(&task2_id)
        .map_err(|e| format!("task-2 should exist: {e}"))?;
    assert_eq!(failed.status.state, TaskState::Failed);

    // Make the worker idle, then quit so the loop exits cleanly.
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .map_err(|_| "worker result send failed")?;
    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "quit send failed")?;

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

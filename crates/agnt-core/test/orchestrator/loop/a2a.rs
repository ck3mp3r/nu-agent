use super::*;

// ---------------------------------------------------------------------------
// A2A task intake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a2a_task_rx_dispatches_turn() -> Result<()> {
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

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);

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
        sources.a2a_task_rx = Some(a2a_task_rx);
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

    a2a_task_tx
        .send(IncomingTask {
            task_id: "task-1".to_string(),
            message: Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "do work".to_string(),
                }],
                message_id: "msg-1".to_string(),
                extensions: None,
                metadata: None,
            },
            sender_url: "http://a.local".to_string(),
            context_id: None,
            parent_task_id: None,
        })
        .await
        .unwrap();

    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "AttachA2aContext should be dispatched")?
        .ok_or("worker channel should not close")?;
    let context_id = match cmd {
        WorkerCommand::AttachA2aContext { context_id, prompt } => {
            assert!(
                prompt.contains("[A2A] do work"),
                "attach must carry the incoming prompt, got: {prompt}"
            );
            context_id
        }
        _ => panic!("expected AttachA2aContext"),
    };
    assert!(
        context_id.len() == 36 && context_id.matches('-').count() == 4,
        "absent contextId must be replaced by a minted UUID, got: {context_id}"
    );

    // The router enqueues the formatted prompt on the UI bus; the TUI answers
    // with a `PromptSubmitted` carrying that same text once the prompt reaches
    // the front of its queue.
    let submitted = format!(
        "[A2A] do work\n\nProcess this request and respond with your answer. Your response will be automatically delivered as the task result.\n\n---\nTask ID: task-1\nFrom: http://a.local\nContext: {context_id}"
    );
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: submitted.clone(),
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
            assert_eq!(prompt, submitted);
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
async fn a2a_task_rx_with_context_id_attaches_that_context() -> Result<()> {
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

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);

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
        sources.a2a_task_rx = Some(a2a_task_rx);
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

    a2a_task_tx
        .send(IncomingTask {
            task_id: "task-1".to_string(),
            message: Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "do work".to_string(),
                }],
                message_id: "msg-1".to_string(),
                extensions: None,
                metadata: None,
            },
            sender_url: "http://a.local".to_string(),
            context_id: Some("ctx-abc".to_string()),
            parent_task_id: None,
        })
        .await
        .unwrap();

    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "AttachA2aContext should be dispatched")?
        .ok_or("worker channel should not close")?;
    let attach_prompt = match cmd {
        WorkerCommand::AttachA2aContext { context_id, prompt } => {
            assert_eq!(context_id, "ctx-abc");
            assert!(
                prompt.contains("[A2A] do work"),
                "attach must carry the incoming prompt, got: {prompt}"
            );
            prompt
        }
        _ => panic!("expected AttachA2aContext"),
    };

    // The router enqueues the formatted prompt on the UI bus; the TUI answers
    // with a `PromptSubmitted` carrying that same text.
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: attach_prompt.clone(),
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
            assert_eq!(prompt, attach_prompt);
            assert!(
                prompt
                    .ends_with("\n\n---\nTask ID: task-1\nFrom: http://a.local\nContext: ctx-abc"),
                "prompt must carry the client contextId, got: {prompt}"
            );
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

// ---------------------------------------------------------------------------
// A2A busy rejection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a2a_task_rx_rejects_second_task_while_worker_busy() -> Result<()> {
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

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);
    let mut turn_rx = bus.turn().subscribe();

    // The server handler transitions an incoming task Submitted→Working before
    // the orchestrator sees it. Model that here so the busy-reject has a
    // Working task to transition.
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
        sources.a2a_task_rx = Some(a2a_task_rx);
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
    // First task: the router attaches the session, then the TUI submits the
    // queued prompt, which dispatches the turn.
    a2a_task_tx
        .send(incoming_task("task-1", "first"))
        .await
        .map_err(|_| "task-1 send failed")?;
    let cmd = recv_worker_command(&mut worker_rx, "AttachA2aContext (task-1)").await?;
    let task1_prompt = match cmd {
        WorkerCommand::AttachA2aContext { prompt, .. } => {
            assert!(
                prompt.contains("[A2A] first"),
                "attach must carry the task-1 prompt, got: {prompt}"
            );
            prompt
        }
        _ => panic!("expected AttachA2aContext for task-1"),
    };
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: task1_prompt.clone(),
        })
        .await
        .map_err(|_| "task-1 submit failed")?;
    let cmd = recv_worker_command(&mut worker_rx, "ExecuteTurn (task-1)").await?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(
                prompt, task1_prompt,
                "task-1 turn prompt must be the submitted text"
            );
        }
        _ => panic!("expected ExecuteTurn for task-1"),
    }
    // Drain task-1's turn-start event so the next one is task-2's.
    let started = tokio::time::timeout(std::time::Duration::from_secs(5), turn_rx.recv())
        .await
        .map_err(|_| "TurnEvent::Started (task-1) should be published")?
        .map_err(|e| format!("turn channel should not close: {e:?}"))?;
    match started {
        TurnEvent::Started { task_id, .. } => {
            assert_eq!(task_id.as_deref(), Some("task-1"));
        }
        _ => panic!("expected TurnEvent::Started for task-1"),
    }

    // Second task arrives while the worker is busy. The orchestrator must reject
    // it instead of dispatching a turn it cannot run.
    a2a_task_tx
        .send(incoming_task(&task2_id, "second"))
        .await
        .map_err(|_| "task-2 send failed")?;
    wait_for_task_state(&store, &task2_id, TaskState::Rejected).await?;
    assert!(
        worker_rx
            .as_mut()
            .ok_or("worker_rx present")?
            .try_recv()
            .is_err(),
        "task-2 must not dispatch a turn while the worker is busy"
    );

    // -- Check
    let rejected = store
        .get_task(&task2_id)
        .map_err(|e| format!("task-2 should exist: {e}"))?;
    assert_eq!(rejected.status.state, TaskState::Rejected);

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

#[tokio::test]
async fn a2a_task_rx_busy_without_store_drops_without_panic() -> Result<()> {
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

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);
    // No task store: the busy branch must log and drop, not panic.
    ctx_state.task_store = None;

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
        sources.a2a_task_rx = Some(a2a_task_rx);
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
    // Dispatch a turn so the worker is busy.
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .map_err(|_| "prompt submit failed")?;
    let _ = recv_worker_command(&mut worker_rx, "ExecuteTurn").await?;

    // A task arrives while the worker is busy and no store is present.
    a2a_task_tx
        .send(incoming_task("task-1", "do work"))
        .await
        .map_err(|_| "task send failed")?;
    // Give the loop a chance to process the arm.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // -- Check
    assert!(
        worker_rx
            .as_mut()
            .ok_or("worker_rx present")?
            .try_recv()
            .is_err(),
        "no command should be dispatched for a busy task without a store"
    );

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

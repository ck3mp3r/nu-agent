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

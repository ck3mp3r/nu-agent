use super::*;

// ---------------------------------------------------------------------------
// ExecuteTurn tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_execute_turn_publishes_exactly_one_started()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut turn_rx = bus.lifecycle().subscribe();
    let sources = dispatch_sources(&bus);
    let mut runtime = AgentSwitchRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let (result_tx, mut result_rx) =
        mpsc::channel::<crate::orchestrator::turn_outcome::TurnResult>(32);

    // -- Exec
    let keep_going = CommandRouter::dispatch(
        WorkerCommand::ExecuteTurn {
            prompt: "hello".to_string(),
            span: Span::test_data(),
            task_id: Some("task-1".to_string()),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(keep_going, "ExecuteTurn must keep the worker loop running");
    let mut started = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        if let crate::bus::LifecycleEvent::TurnStarted { prompt, task_id } = event {
            started.push((prompt, task_id));
        }
    }
    assert_eq!(
        started,
        vec![("hello".to_string(), Some("task-1".to_string()))],
        "the router must publish exactly one attributed LifecycleEvent::TurnStarted"
    );
    let outcome = result_rx
        .recv()
        .await
        .ok_or("the router must send the turn outcome")?;
    // The result carries the attribution the turn was dispatched under.
    assert_eq!(
        outcome.task_id.as_deref(),
        Some("task-1"),
        "the router must bind the command's task id to the turn result"
    );
    assert!(
        matches!(
            outcome.outcome,
            crate::orchestrator::turn_outcome::TurnOutcome::Success(_)
        ),
        "the turn must report success"
    );
    Ok(())
}

/// WHEN a turn is cancelled, THE router's ExecuteTurn arm SHALL construct the
/// result with `TurnOutcome::Cancelled` from the turn end's typed variant — no
/// message text decides it.
#[tokio::test]
async fn dispatch_execute_turn_maps_a_cancelled_turn_end_to_cancelled()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let sources = dispatch_sources(&bus);
    let mut runtime = AgentSwitchRuntime {
        bus: bus.clone(),
        turn_end: Some(TurnEnd::Cancelled {
            value: Value::nothing(Span::test_data()),
        }),
        ..Default::default()
    };
    let (result_tx, mut result_rx) = mpsc::channel::<TurnResult>(32);

    // -- Exec
    let keep_going = CommandRouter::dispatch(
        WorkerCommand::ExecuteTurn {
            prompt: "hello".to_string(),
            span: Span::test_data(),
            task_id: Some("task-1".to_string()),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(keep_going, "ExecuteTurn must keep the worker loop running");
    let outcome = result_rx
        .recv()
        .await
        .ok_or("the router must send the turn outcome")?;
    assert!(
        matches!(outcome.outcome, TurnOutcome::Cancelled),
        "a cancelled turn end must reach the consumer as Cancelled; got {:?}",
        outcome.outcome
    );
    // The attribution survives the cancelled mapping.
    assert_eq!(outcome.task_id.as_deref(), Some("task-1"));
    Ok(())
}

// ---------------------------------------------------------------------------
// dispatch_ui_request tests
// ---------------------------------------------------------------------------
#[tokio::test]
async fn dispatch_ui_request_switch_model() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchModel {
            spec: "test-model".to_string(),
        },
        &mut runtime,
        &None::<NoAgentSwitchHook>,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::ModelSwitch(result) => {
            assert!(
                result.is_err(),
                "AgentSwitchRuntime switch_model returns Err"
            );
        }
        other => panic!("expected ModelSwitch, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_switch_agent() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchAgent {
            name: "research-agent".to_string(),
        },
        &mut runtime,
        &None::<NoAgentSwitchHook>,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::AgentSwitch(result) => {
            let (agent, model, _max_tokens, _icon) =
                result.map_err(|e| format!("switch should succeed: {e:?}"))?;
            assert_eq!(agent, "research-agent");
            assert_eq!(model, "openai/gpt-4o-mini");
        }
        other => panic!("expected AgentSwitch, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_switch_agent_invokes_callback()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let hook = RecordingAgentSwitchHook::default();
    let hook_calls = Arc::clone(&hook.calls);
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    // -- Exec
    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchAgent {
            name: "research-agent".to_string(),
        },
        &mut runtime,
        &Some(hook),
        response_tx,
    )
    .await;

    // -- Check
    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::AgentSwitch(result) => {
            assert!(result.is_ok(), "switch should succeed");
        }
        other => panic!("expected AgentSwitch, got {other:?}"),
    }

    let calls = hook_calls.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(calls.len(), 1, "the hook must fire exactly once");
    assert_eq!(calls[0].0, "research-agent");
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_switch_session() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let loaded_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = SwitchSessionRuntime {
        loaded_session_id: Arc::clone(&loaded_session_id),
        attached_session_id: None,
        resolved_session_id: None,
        active_model: "openai/gpt-4o-mini".to_string(),
        bus: crate::bus::Bus::default(),
    };
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchSession {
            id: "test-session-123".to_string(),
        },
        &mut runtime,
        &None::<NoAgentSwitchHook>,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::SessionSwitch { id, result } => {
            assert_eq!(id, "test-session-123");
            assert!(result.is_err(), "default load_session returns Err");
        }
        other => panic!("expected SessionSwitch, got {other:?}"),
    }

    let loaded = loaded_session_id.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(loaded.as_deref(), Some("test-session-123"));
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_toggle_mcp() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::ToggleMcp {
            server: "test-server".to_string(),
            enable: true,
        },
        &mut runtime,
        &None::<NoAgentSwitchHook>,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::McpToggle {
            server,
            result,
            total,
            server_count,
            names_by_server,
        } => {
            assert_eq!(server, "test-server");
            assert_eq!(result, Ok(McpUsabilityState::Disabled));
            assert_eq!(total, 0);
            assert_eq!(server_count, 0);
            assert!(names_by_server.is_empty());
        }
        other => panic!("expected McpToggle, got {other:?}"),
    }
    Ok(())
}

use super::*;

// ---------------------------------------------------------------------------
// ExecuteTurn tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_execute_turn_publishes_exactly_one_started()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut turn_rx = bus.turn().subscribe();
    let mut runtime = AgentSwitchRuntime::default();
    let (result_tx, mut result_rx) =
        mpsc::channel::<crate::orchestrator::turn_outcome::TurnOutcome>(32);

    // -- Exec
    let keep_going = CommandRouter::dispatch(
        WorkerCommand::ExecuteTurn {
            prompt: "hello".to_string(),
            span: Span::test_data(),
            task_id: Some("task-1".to_string()),
        },
        &mut runtime,
        &result_tx,
        None,
        &bus,
    )
    .await;

    // -- Check
    assert!(keep_going, "ExecuteTurn must keep the worker loop running");
    let mut started = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        if let crate::bus::TurnEvent::Started { prompt, task_id } = event {
            started.push((prompt, task_id));
        }
    }
    assert_eq!(
        started,
        vec![("hello".to_string(), Some("task-1".to_string()))],
        "the router must publish exactly one attributed TurnEvent::Started"
    );
    let outcome = result_rx
        .recv()
        .await
        .ok_or("the router must send the turn outcome")?;
    assert!(
        matches!(
            outcome,
            crate::orchestrator::turn_outcome::TurnOutcome::Success(_)
        ),
        "the turn must report success"
    );
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
        &None,
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
        &None,
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
    let switched_name = Arc::new(Mutex::new(None::<String>));
    let cb_name = Arc::clone(&switched_name);
    let callback: OnAgentSwitch = Arc::new(
        move |name: String, _desc: Option<String>, _icon: Option<String>| {
            *cb_name.lock().expect("name lock") = Some(name);
        },
    );

    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchAgent {
            name: "research-agent".to_string(),
        },
        &mut runtime,
        &Some(callback),
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::AgentSwitch(result) => {
            assert!(result.is_ok(), "switch should succeed");
        }
        other => panic!("expected AgentSwitch, got {other:?}"),
    }

    let name = switched_name.lock().expect("name lock").take();
    assert_eq!(name.as_deref(), Some("research-agent"));
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_switch_session() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let loaded_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = SwitchSessionRuntime {
        loaded_session_id: Arc::clone(&loaded_session_id),
        active_model: "openai/gpt-4o-mini".to_string(),
    };
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchSession {
            id: "test-session-123".to_string(),
        },
        &mut runtime,
        &None,
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

    let loaded = loaded_session_id.lock().expect("lock");
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
        &None,
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

#[tokio::test]
async fn dispatch_ui_request_refresh_agent_picker()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::RefreshAgentPicker,
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::AgentRefresh(result) => {
            let summaries = result.map_err(|e| format!("scan should succeed: {e}"))?;
            assert!(
                summaries.is_empty(),
                "a mock runtime's default scan returns no personas"
            );
        }
        other => panic!("expected AgentRefresh, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn dispatch_ui_request_refresh_session_picker()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::RefreshSessionPicker,
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::SessionRefresh(result) => {
            assert!(result.is_ok(), "default list_sessions returns Ok");
        }
        other => panic!("expected SessionRefresh, got {other:?}"),
    }
    Ok(())
}

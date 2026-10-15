use super::*;

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

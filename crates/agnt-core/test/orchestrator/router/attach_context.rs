use super::*;

// ---------------------------------------------------------------------------
// AttachA2aContext — derived session key
// ---------------------------------------------------------------------------

struct AttachContextRuntime {
    attached_session_id: Arc<Mutex<Option<String>>>,
    cwd: std::path::PathBuf,
    snapshots: Vec<UiMessageSnapshot>,
    current_session_id: Option<String>,
}

impl CoreRuntime for AttachContextRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ModelSwitching for AttachContextRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for AttachContextRuntime {}

impl SessionPersistence for AttachContextRuntime {
    fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    fn current_session_id(&self) -> Option<&str> {
        self.current_session_id.as_deref()
    }

    async fn attach_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        self.attached_session_id
            .lock()
            .expect("attached_session_id lock")
            .replace(session_id.to_string());
        self.current_session_id = Some(session_id.to_string());
        Ok(self.snapshots.clone())
    }
}

crate::default_mcp!(AttachContextRuntime);

#[tokio::test]
async fn attach_a2a_context_derives_prefixed_session_key()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cwd = std::path::PathBuf::from("/home/user/project");
    let expected_prefix = crate::session::prefix::dir_prefix(&cwd);
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: Vec::new(),
        current_session_id: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = create_bus();
    let mut session_rx = bus.session().subscribe();

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-abc".to_string(),
            prompt: "[A2A] hello".to_string(),
        },
        &mut runtime,
        &result_tx,
        None,
        &bus,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let expected_key = format!("{expected_prefix}-ctx-abc");
    let attached = attached_session_id.lock().expect("lock").clone();
    assert_eq!(
        attached.as_deref(),
        Some(expected_key.as_str()),
        "attach_session must receive the derived {{prefix}}-{{contextId}} key"
    );
    let event = session_rx
        .try_recv()
        .map_err(|e| format!("session event should be published: {e:?}"))?;
    match event {
        crate::bus::SessionEvent::Switched { to_session_id, .. } => {
            assert_eq!(to_session_id, expected_key);
        }
        other => panic!("expected SessionEvent::Switched, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn attach_a2a_context_clears_transcript_when_snapshots_empty()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cwd = std::path::PathBuf::from("/home/user/project");
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: Vec::new(),
        current_session_id: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = create_bus();
    let mut ui_state_rx = bus.ui_state().subscribe();

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-fresh".to_string(),
            prompt: "[A2A] fresh task".to_string(),
        },
        &mut runtime,
        &result_tx,
        None,
        &bus,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("ui state event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::ClearTranscript => {}
        _ => panic!("expected UiStateEvent::ClearTranscript for an empty snapshot list"),
    }
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("startup logo event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::PushStartupLogo => {}
        _ => panic!(
            "expected UiStateEvent::PushStartupLogo after the clear; a fresh session must mirror the /new visual"
        ),
    }
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("incoming message event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::EnqueueExternalPrompt { text: message } => {
            assert_eq!(message, "[A2A] fresh task");
        }
        _ => panic!(
            "expected UiStateEvent::EnqueueExternalPrompt after the startup logo; an empty snapshot list must not hydrate a transcript"
        ),
    }
    assert!(
        ui_state_rx.try_recv().is_err(),
        "a fresh session must not send any further ui state event"
    );
    Ok(())
}

#[tokio::test]
async fn attach_a2a_context_hydrates_transcript_when_snapshots_present()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cwd = std::path::PathBuf::from("/home/user/project");
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: vec![UiMessageSnapshot::new("user", "hello")],
        current_session_id: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = create_bus();
    let mut ui_state_rx = bus.ui_state().subscribe();

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-resumed".to_string(),
            prompt: "[A2A] resumed task".to_string(),
        },
        &mut runtime,
        &result_tx,
        None,
        &bus,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("ui state event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::ClearTranscript => {}
        _ => panic!("expected UiStateEvent::ClearTranscript before the transcript hydrate"),
    }
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("ui state event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::HydrateTranscript {
            messages,
            last_total_tokens,
        } => {
            assert_eq!(messages.len(), 1, "one snapshot should be hydrated");
            assert_eq!(last_total_tokens, None);
        }
        _ => panic!("expected UiStateEvent::HydrateTranscript for a non-empty snapshot list"),
    }
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("incoming message event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::EnqueueExternalPrompt { text: message } => {
            assert_eq!(message, "[A2A] resumed task");
        }
        _ => panic!("expected UiStateEvent::EnqueueExternalPrompt after the transcript hydrate"),
    }
    Ok(())
}

#[tokio::test]
async fn attach_a2a_context_same_session_skips_clear_and_hydrate()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cwd = std::path::PathBuf::from("/home/user/project");
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let active_key = format!("{prefix}-ctx-same");
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: vec![UiMessageSnapshot::new("user", "hello")],
        current_session_id: Some(active_key),
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = create_bus();
    let mut ui_state_rx = bus.ui_state().subscribe();
    let mut session_rx = bus.session().subscribe();

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-same".to_string(),
            prompt: "[A2A] same session task".to_string(),
        },
        &mut runtime,
        &result_tx,
        None,
        &bus,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let event = ui_state_rx
        .try_recv()
        .map_err(|e| format!("enqueue event should be published: {e:?}"))?;
    match event {
        crate::orchestrator::UiStateEvent::EnqueueExternalPrompt { text } => {
            assert_eq!(text, "[A2A] same session task");
        }
        _ => panic!("expected UiStateEvent::EnqueueExternalPrompt as the only ui state event"),
    }
    assert!(
        ui_state_rx.try_recv().is_err(),
        "a same-session attach must not send ClearTranscript, PushStartupLogo, or HydrateTranscript"
    );
    assert!(
        session_rx.try_recv().is_err(),
        "a same-session attach must not re-emit SessionEvent::Switched"
    );
    assert_eq!(
        attached_session_id.lock().expect("lock").clone(),
        None,
        "a same-session attach must not call attach_session"
    );
    Ok(())
}

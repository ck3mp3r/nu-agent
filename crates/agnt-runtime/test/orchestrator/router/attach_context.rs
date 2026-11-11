use super::*;

use crate::protocol::contracts::UiMessageUsageSnapshot;

// ---------------------------------------------------------------------------
// AttachA2aContext — successful attach
// ---------------------------------------------------------------------------

/// One assistant row reporting the wire total it was persisted with.
fn assistant_row_with_total(total_tokens: u64) -> UiMessageSnapshot {
    let mut row = UiMessageSnapshot::new("assistant", "hi");
    row.usage = Some(UiMessageUsageSnapshot {
        input_tokens: Some(11),
        output_tokens: Some(4),
        total_tokens: Some(total_tokens),
    });
    row
}

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
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let sources = dispatch_sources(&bus);

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-abc".to_string(),
            prompt: "[A2A] hello".to_string(),
            task_id: "task-abc".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let expected_key = format!("{expected_prefix}-ctx-abc");
    let attached = attached_session_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert_eq!(
        attached.as_deref(),
        Some(expected_key.as_str()),
        "attach_session must receive the derived {{prefix}}-{{contextId}} key"
    );
    let event = lifecycle_rx
        .try_recv()
        .map_err(|e| format!("session event should be published: {e:?}"))?;
    match event {
        crate::bus::LifecycleEvent::SessionSwitched { to, .. } => {
            assert_eq!(to, expected_key);
        }
        other => panic!("expected LifecycleEvent::SessionSwitched, got {other:?}"),
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
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let mut control_rx = bus.control().subscribe();
    let mut stream_rx = bus.stream().subscribe();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let sources = dispatch_sources(&bus);

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-fresh".to_string(),
            prompt: "[A2A] fresh task".to_string(),
            task_id: "task-fresh".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    // The switch marker opens the record log: the surface derives the clear
    // from it, so no separate clear/logo event exists.
    let record = stream_rx
        .try_recv()
        .map_err(|e| format!("the switch marker should be published: {e:?}"))?;
    match record {
        SessionRecord::Session(SessionLifecycle::Switched { .. }) => {}
        other => panic!("expected the session switch marker, got {other:?}"),
    }
    // The boundary's own prompt row follows. An empty snapshot list replays no
    // transcript rows, and the row that arrives is the arrival's own.
    let record = stream_rx
        .try_recv()
        .map_err(|e| format!("the prompt row should be published: {e:?}"))?;
    match record {
        SessionRecord::Prompt(prompt) => {
            assert_eq!(prompt.text, "[A2A] fresh task");
            assert_eq!(prompt.task_id.as_deref(), Some("task-fresh"));
            assert!(
                prompt.stamps.millis.is_some(),
                "the accepting boundary stamps the row's intake instant"
            );
        }
        other => panic!("expected the prompt row after the marker, got {other:?}"),
    }
    assert!(
        stream_rx.try_recv().is_err(),
        "an empty snapshot list must replay no transcript rows"
    );
    let event = lifecycle_rx
        .try_recv()
        .map_err(|e| format!("the session switch should be announced: {e:?}"))?;
    match event {
        crate::bus::LifecycleEvent::SessionSwitched { .. } => {}
        other => panic!("expected LifecycleEvent::SessionSwitched, got {other:?}"),
    }
    let command = control_rx
        .try_recv()
        .map_err(|e| format!("incoming message command should be published: {e:?}"))?;
    match command {
        crate::bus::ControlCommand::EnqueueExternalPrompt { task_id, prompt } => {
            assert_eq!(prompt, "[A2A] fresh task");
            assert_eq!(
                task_id, "task-fresh",
                "the enqueued prompt must travel with its task id"
            );
        }
        other => panic!("expected EnqueueExternalPrompt after the replay, got {other:?}"),
    }
    assert!(
        control_rx.try_recv().is_err(),
        "a fresh session must not send any further control command"
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
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let mut control_rx = bus.control().subscribe();
    let mut stream_rx = bus.stream().subscribe();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let sources = dispatch_sources(&bus);

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-resumed".to_string(),
            prompt: "[A2A] resumed task".to_string(),
            task_id: "task-resumed".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    // The marker opens the replay; the stored rows follow it in order, so the
    // surface replaces its transcript with exactly what was stored.
    let record = stream_rx
        .try_recv()
        .map_err(|e| format!("the switch marker should be published: {e:?}"))?;
    match record {
        SessionRecord::Session(SessionLifecycle::Switched { .. }) => {}
        other => panic!("expected the session switch marker, got {other:?}"),
    }
    let record = stream_rx
        .try_recv()
        .map_err(|e| format!("the replayed row should be published: {e:?}"))?;
    match record {
        SessionRecord::Prompt(prompt) => {
            assert_eq!(prompt.text, "hello", "the stored prompt must replay");
        }
        other => panic!("expected a replayed prompt record, got {other:?}"),
    }
    let event = lifecycle_rx
        .try_recv()
        .map_err(|e| format!("the session switch should be announced: {e:?}"))?;
    match event {
        crate::bus::LifecycleEvent::SessionSwitched { .. } => {}
        other => panic!("expected LifecycleEvent::SessionSwitched, got {other:?}"),
    }
    let command = control_rx
        .try_recv()
        .map_err(|e| format!("incoming message command should be published: {e:?}"))?;
    match command {
        crate::bus::ControlCommand::EnqueueExternalPrompt { task_id, prompt } => {
            assert_eq!(prompt, "[A2A] resumed task");
            assert_eq!(
                task_id, "task-resumed",
                "the resumed prompt keeps its task id"
            );
        }
        other => panic!("expected EnqueueExternalPrompt after the replay, got {other:?}"),
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
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let mut control_rx = bus.control().subscribe();
    let mut stream_rx = bus.stream().subscribe();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let sources = dispatch_sources(&bus);

    // -- Exec
    let should_continue = CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-same".to_string(),
            prompt: "[A2A] same session task".to_string(),
            task_id: "task-same".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let command = control_rx
        .try_recv()
        .map_err(|e| format!("enqueue command should be published: {e:?}"))?;
    match command {
        crate::bus::ControlCommand::EnqueueExternalPrompt { task_id, prompt } => {
            assert_eq!(prompt, "[A2A] same session task");
            assert_eq!(
                task_id, "task-same",
                "the same-session prompt keeps its task id"
            );
        }
        other => panic!("expected EnqueueExternalPrompt as the only command, got {other:?}"),
    }
    assert!(
        control_rx.try_recv().is_err(),
        "a same-session attach must not send a second command"
    );
    // The stream carries the arrival's own prompt row and nothing else: a
    // same-session attach replays no transcript and publishes no marker.
    let record = stream_rx
        .try_recv()
        .map_err(|e| format!("the prompt row should be published: {e:?}"))?;
    match record {
        SessionRecord::Prompt(prompt) => {
            assert_eq!(prompt.text, "[A2A] same session task");
            assert_eq!(prompt.task_id.as_deref(), Some("task-same"));
        }
        other => panic!("expected only the prompt row on the stream, got {other:?}"),
    }
    assert!(
        stream_rx.try_recv().is_err(),
        "a same-session attach must not replay the transcript"
    );
    assert!(
        lifecycle_rx.try_recv().is_err(),
        "a same-session attach must not re-emit SessionSwitched"
    );
    assert_eq!(
        attached_session_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        None,
        "a same-session attach must not call attach_session"
    );
    Ok(())
}

/// WHEN the router finishes an attach whose stored rows carry a token count, THE
/// runtime SHALL receive `seed_last_total_tokens` with exactly that count.
///
/// A switch replaces the transcript the context window is measured from, so the
/// fresh session must start from the count the stored rows report instead of
/// from the previous session's last total.
#[tokio::test]
async fn attach_a2a_context_seeds_the_stored_token_count()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures: one assistant row reporting its wire total.
    let cwd = std::path::PathBuf::from("/home/user/project");
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: vec![
            UiMessageSnapshot::new("user", "hello"),
            assistant_row_with_total(15),
        ],
        current_session_id: None,
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let sources = dispatch_sources(&bus);

    // -- Exec
    CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-resumed".to_string(),
            prompt: "[A2A] resumed task".to_string(),
            task_id: "task-resumed".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert_eq!(
        runtime.seeded_tokens,
        Some(Some(15)),
        "the attach must seed exactly the token count its stored rows carry"
    );
    Ok(())
}

/// WHEN the attach finds the session already current, THE router SHALL skip the
/// clear/hydrate cycle and SHALL NOT reseed the runtime.
#[tokio::test]
async fn same_session_attach_does_not_reseed()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let cwd = std::path::PathBuf::from("/home/user/project");
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let active_key = format!("{prefix}-ctx-same");
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: vec![assistant_row_with_total(15)],
        current_session_id: Some(active_key),
        seeded_tokens: None,
        attach_error: None,
    };
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);
    let bus = Bus::default();
    let sources = dispatch_sources(&bus);

    // -- Exec
    CommandRouter::dispatch(
        WorkerCommand::AttachA2aContext {
            context_id: "ctx-same".to_string(),
            prompt: "[A2A] same session task".to_string(),
            task_id: "task-same".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check: the transcript was not replayed, so the count was not replaced.
    assert_eq!(
        runtime.seeded_tokens, None,
        "a same-session attach must not reseed the token count"
    );
    Ok(())
}

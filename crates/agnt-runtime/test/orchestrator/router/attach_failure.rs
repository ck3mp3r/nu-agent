//! AttachA2aContext — the failed attach.
//!
//! A failed attach is a failed task: the caller moved the task to `Working` at
//! the boundary and waits for a terminal state, so the failure travels the
//! task-outcome spine and the prompt is never enqueued.

use super::*;

/// IF the session attach fails, THEN THE `CommandRouter::dispatch` SHALL publish
/// exactly one `LifecycleEvent::TaskFailed` naming that task and SHALL publish
/// no `SessionRecord::Prompt` and no `ControlCommand::EnqueueExternalPrompt`.
#[tokio::test]
async fn attach_a2a_context_failure_fails_the_task_without_enqueueing()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures: an attach that reports the store refused the key.
    let cwd = std::path::PathBuf::from("/home/user/project");
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let attached_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = AttachContextRuntime {
        attached_session_id: Arc::clone(&attached_session_id),
        cwd,
        snapshots: Vec::new(),
        current_session_id: None,
        seeded_tokens: None,
        attach_error: Some("session store unavailable".to_string()),
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
            context_id: "ctx-broken".to_string(),
            prompt: "[A2A] doomed task".to_string(),
            task_id: "task-broken".to_string(),
        },
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &sources,
    )
    .await;

    // -- Check
    assert!(should_continue, "AttachA2aContext should not shut down");
    let event = lifecycle_rx
        .try_recv()
        .map_err(|e| format!("the attach failure should fail the task: {e:?}"))?;
    match event {
        LifecycleEvent::TaskFailed { task_id, error } => {
            assert_eq!(task_id, "task-broken");
            assert_eq!(
                error, "session store unavailable",
                "the row carries the attach failure's own text"
            );
        }
        other => return Err(format!("expected LifecycleEvent::TaskFailed, got {other:?}").into()),
    }
    assert!(
        lifecycle_rx.try_recv().is_err(),
        "a failed attach must not also announce a session switch"
    );
    assert!(
        stream_rx.try_recv().is_err(),
        "a failed attach must not publish the prompt row into the current session"
    );
    assert!(
        control_rx.try_recv().is_err(),
        "a failed attach must not enqueue the prompt"
    );
    // The attach was attempted on the derived key and the runtime was left on
    // the session it already had.
    assert_eq!(
        attached_session_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        Some(format!("{prefix}-ctx-broken")),
        "the attach must be attempted on the derived {{prefix}}-{{contextId}} key"
    );
    assert_eq!(
        runtime.current_session_id, None,
        "a failed attach must not switch the runtime's session"
    );
    Ok(())
}

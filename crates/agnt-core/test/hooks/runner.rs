//! Turn and tool event mapping, filtering, ordering, and failure handling.

use super::*;

use crate::session::tool_status::ToolStatus;

#[tokio::test]
async fn test_hooks_runner_turn_started_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: Some("task-1".to_string()),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "one event must invoke the hook once");
    assert_eq!(calls[0].event, "turn_started");
    assert_eq!(calls[0].script, PathBuf::from("/hooks/audit.nu"));
    assert_eq!(calls[0].cwd, session_cwd());
    assert_eq!(calls[0].timeout, Duration::from_millis(10_000));
    let body = payload(&calls[0])?;
    assert_eq!(body["prompt"], "hello");
    assert_eq!(body["task_id"], "task-1");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_turn_completed_invokes_with_tool_calls() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 4,
            input_tokens: 30,
            output_tokens: 12,
            total_tokens: Some(42),
            cost: Some(0.3),
            cost_complete: false,
            duration_ms: 1_500,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "turn_completed");
    let body = payload(&calls[0])?;
    assert_eq!(body["tool_calls"], 4);
    assert_eq!(body["input_tokens"], 30);
    assert_eq!(body["output_tokens"], 12);
    assert_eq!(body["total_tokens"], 42);
    assert_eq!(body["cost"], 0.3);
    assert_eq!(body["cost_complete"], false);
    assert_eq!(body["duration_ms"], 1_500);
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_tool_started_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ToolStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("read".to_string()),
                "builtin",
                "{\"path\":\"a.txt\"}",
                crate::protocol::tool_args::CallLine::from_json_summary("{}"),
            ),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "tool_started");
    let body = payload(&calls[0])?;
    assert_eq!(body["name"], "read");
    assert_eq!(body["source"], "builtin");
    assert_eq!(body["arguments"], "{\"path\":\"a.txt\"}");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_tool_completed_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ToolCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(SystemEvent::ToolCompleted {
            call: ToolCall::new(
                ToolName("read".to_string()),
                "builtin",
                "{}",
                crate::protocol::tool_args::CallLine::from_json_summary("{}"),
            ),
            outcome: ToolOutcome {
                success: false,
                status: ToolStatus::Failed,
                result: "denied".to_string(),
                display: None,
                error_kind: Some("permission".to_string()),
                message: Some("not allowed".to_string()),
            },
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].event, "tool_completed");
    let body = payload(&calls[0])?;
    assert_eq!(body["name"], "read");
    assert_eq!(body["source"], "builtin");
    assert_eq!(body["arguments"], "{}");
    assert_eq!(body["success"], false);
    assert_eq!(body["error_kind"], "permission");
    assert_eq!(body["message"], "not allowed");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_skips_unsubscribed_events() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: None,
        })
        .await;
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the subscribed event must invoke the hook; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "turn_completed");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_invokes_in_arrival_order() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[HookEvent::TurnStarted, HookEvent::TurnCompleted],
    )];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "first".to_string(),
            task_id: None,
        })
        .await;
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 2,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 2, "both events must invoke the hook");
    assert_eq!(calls[0].event, "turn_started");
    assert_eq!(calls[1].event, "turn_completed");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_continues_after_a_timeout() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::timing_out();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 2,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "a timed-out hook must not stop the loop; got: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_continues_after_a_failure() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::failing();
    let hooks = vec![hook("audit", &[HookEvent::TurnCompleted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    let _ = bus
        .ui_event()
        .send(SystemEvent::TurnCompleted {
            tool_calls: 2,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "a failed hook must not stop the loop; got: {calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_failure_warning_names_the_hook() -> Result<()> {
    // -- Setup & Fixtures
    let error = HookProcessError::Exit {
        exit_code: 3,
        stderr: "no such function".to_string(),
    };

    // -- Exec
    let warning = crate::hooks::failure_warning("audit", "turn_completed", &error);

    // -- Check
    assert!(
        warning.contains("audit"),
        "the warning must name the hook: {warning}"
    );
    assert!(
        warning.contains("turn_completed"),
        "the warning must name the event: {warning}"
    );
    assert!(
        warning.contains("no such function"),
        "the warning must carry the process error: {warning}"
    );
    Ok(())
}

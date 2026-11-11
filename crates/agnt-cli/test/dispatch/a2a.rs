use super::*;

use std::io::IsTerminal;

use agnt_a2a::{
    A2aCompletionEvent, InMemoryTaskStore, IncomingTask, Message, Part, Role, TaskState,
};
use agnt_runtime::orchestrator::external::ExternalTaskStore;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An incoming task from a remote agent.
fn incoming_task(context_id: Option<&str>) -> IncomingTask {
    IncomingTask {
        task_id: "task-1".to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "hello peer".to_string(),
            }],
            message_id: "msg-1".to_string(),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://peer".to_string(),
        context_id: context_id.map(str::to_string),
        parent_task_id: None,
    }
}

/// A context that owns the raw task and completion channels.
fn context_with_channels(
    task_rx: tokio::sync::mpsc::Receiver<IncomingTask>,
    completion_rx: tokio::sync::mpsc::Receiver<A2aCompletionEvent>,
) -> A2aContext {
    A2aContext {
        task_rx: Some(task_rx),
        completion_rx: Some(completion_rx),
        ..Default::default()
    }
}

#[test]
fn test_dispatch_a2a_resolve_mode_matches_terminal_state() {
    // -- Setup & Fixtures
    // resolve_mode() always passes input_is_nothing=true, so TUI is selected
    // only when both stdin and stderr are terminals.
    let both_tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();

    // -- Exec
    let mode = resolve_mode();

    // -- Check
    if both_tty {
        assert_eq!(mode, CliMode::Tui);
    } else {
        assert_eq!(mode, CliMode::Stderr);
    }
}

#[tokio::test]
async fn test_dispatch_a2a_bridge_forwards_task_context_and_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let (task_tx, task_rx) = tokio::sync::mpsc::channel(4);
    let (_completion_tx, completion_rx) = tokio::sync::mpsc::channel(4);
    let mut a2a = context_with_channels(task_rx, completion_rx);
    assert!(task_tx.send(incoming_task(Some("ctx-abc"))).await.is_ok());

    // -- Exec
    a2a.bridge_a2a_channels();
    let task = a2a
        .external_task_rx
        .as_mut()
        .ok_or("bridge should install an external task receiver")?
        .recv()
        .await
        .ok_or("bridge should forward the queued task")?;

    // -- Check
    assert_eq!(task.task_id, "task-1");
    assert_eq!(task.context_id, "ctx-abc");
    assert!(task.prompt.contains("hello peer"));
    assert!(task.prompt.contains("Context: ctx-abc"));
    Ok(())
}

/// A task with no carried context id cannot happen: the server's send handlers
/// resolve one before publishing. If it does happen, the bridge must drop the
/// task rather than mint a context the loop's session attach would not name.
#[tokio::test]
async fn test_dispatch_a2a_bridge_drops_a_task_without_a_carried_context_id() -> Result<()> {
    // -- Setup & Fixtures
    let (task_tx, task_rx) = tokio::sync::mpsc::channel(4);
    let (_completion_tx, completion_rx) = tokio::sync::mpsc::channel(4);
    let mut a2a = context_with_channels(task_rx, completion_rx);
    assert!(task_tx.send(incoming_task(None)).await.is_ok());

    // -- Exec
    a2a.bridge_a2a_channels();
    let forwarded = a2a
        .external_task_rx
        .as_mut()
        .ok_or("bridge should install an external task receiver")?
        .recv()
        .await;

    // -- Check
    assert!(
        forwarded.is_none(),
        "the bridge must not mint a context id of its own"
    );
    Ok(())
}

#[tokio::test]
async fn test_dispatch_a2a_bridge_forwards_completion_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let (_task_tx, task_rx) = tokio::sync::mpsc::channel(4);
    let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(4);
    let mut a2a = context_with_channels(task_rx, completion_rx);
    let event = A2aCompletionEvent {
        task_id: "task-42".to_string(),
        agent_name: "peer".to_string(),
        status: TaskState::Completed,
        context_id: Some("ctx-9".to_string()),
    };
    let expected = event.to_prompt();
    assert!(completion_tx.send(event).await.is_ok());

    // -- Exec
    a2a.bridge_a2a_channels();
    let completion = a2a
        .external_completion_rx
        .as_mut()
        .ok_or("bridge should install an external completion receiver")?
        .recv()
        .await
        .ok_or("bridge should forward the queued completion")?;

    // -- Check
    assert_eq!(completion.prompt, expected);
    Ok(())
}

#[test]
fn test_dispatch_a2a_gate_rejects_through_the_raw_store() -> Result<()> {
    // -- Setup & Fixtures
    let store = Arc::new(InMemoryTaskStore::default());
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    let gate = crate::mode::A2aExternalTaskGate::new(Arc::clone(&store));

    // -- Exec
    let rejected = gate.reject_task(&task.id, "worker busy");
    let unknown = gate.reject_task("unknown-task", "worker busy");

    // -- Check
    assert!(rejected.is_ok());
    assert!(unknown.is_err());
    assert_eq!(
        store
            .get_task(&task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Rejected
    );
    Ok(())
}

#[test]
fn test_dispatch_a2a_peer_tool_definitions_map_registry_one_to_one() -> Result<()> {
    // -- Setup & Fixtures
    let registry = agnt_a2a::a2a_tool_defs();

    // -- Exec
    let definitions = peer_tool_definitions(true)?;
    let disabled = peer_tool_definitions(false)?;

    // -- Check
    assert_eq!(definitions.len(), registry.len());
    for (definition, expected) in definitions.iter().zip(registry.iter()) {
        assert_eq!(definition.name.as_str(), expected.name.as_str());
        assert_eq!(definition.description, expected.description);
        assert_eq!(definition.parameters, expected.parameters);
    }

    let names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();
    for expected in ["tasks_send", "tasks_get", "tasks_list"] {
        assert!(
            names.contains(&expected),
            "{expected} must be advertised, got: {names:?}"
        );
    }
    assert!(disabled.is_empty());
    Ok(())
}

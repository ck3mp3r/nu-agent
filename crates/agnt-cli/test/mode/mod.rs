use super::*;

use agnt_a2a::{InMemoryTaskStore, Task, TaskState};
use agnt_core::bus::{AgentEvent, AgentTx};
use agnt_core::config::{HookEvent, ResolvedHook};
use nu_protocol::Span;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Records every hook invocation and always succeeds.
#[derive(Clone, Default)]
struct RecordingProcess {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl RecordingProcess {
    fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl agnt_core::hooks::HookProcess for RecordingProcess {
    async fn run(
        &self,
        _script: &std::path::Path,
        event: &str,
        envelope: &str,
        _cwd: &std::path::Path,
        _timeout: std::time::Duration,
    ) -> core::result::Result<(), agnt_core::hooks::HookProcessError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((event.to_string(), envelope.to_string()));
        }
        Ok(())
    }
}

/// A resolved hook subscribing to `events`, with a script path that is never run.
fn resolved_hook(name: &str, events: &[HookEvent]) -> ResolvedHook {
    ResolvedHook {
        name: name.to_string(),
        events: events.to_vec(),
        script: std::path::PathBuf::from(format!("/hooks/{name}.nu")),
        timeout: std::time::Duration::from_secs(10),
    }
}

/// Create a task and move it to `Working`, the state a task is in while a turn
/// runs. Only `Working` tasks can transition to `Completed` or `Failed`.
fn working_task(store: &InMemoryTaskStore) -> Result<Task> {
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    Ok(task)
}

// endregion: --- Test Support

#[test]
fn test_mode_spawn_configured_hooks_without_a_config_spawns_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();

    // -- Exec
    let handles = spawn_configured_hooks(None, &bus, std::path::Path::new("/tmp"), process.clone());

    // -- Check
    assert!(
        handles.is_empty(),
        "no plugin config must spawn no hook runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[test]
fn test_mode_spawn_configured_hooks_with_empty_hooks_spawns_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();
    let config = PluginConfig::default();

    // -- Exec
    let handles = spawn_configured_hooks(
        Some(&config),
        &bus,
        std::path::Path::new("/tmp"),
        process.clone(),
    );

    // -- Check
    assert!(
        handles.is_empty(),
        "an empty hooks config must spawn no hook runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[test]
fn test_mode_auto_complete_a2a_task_sets_completed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;

    // -- Exec
    auto_complete_a2a_task(&store, &task.id, &Value::string("done", Span::unknown()));

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Completed);
    Ok(())
}

#[test]
fn test_mode_auto_fail_a2a_task_sets_failed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = working_task(&store)?;

    // -- Exec
    auto_fail_a2a_task(&store, &task.id, "boom");

    // -- Check
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Failed);
    Ok(())
}

#[test]
fn test_mode_apply_turn_event_to_store_completed_and_failed() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let completed = working_task(&store)?;
    let failed = working_task(&store)?;

    // -- Exec
    apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskCompleted {
            output: "ok".to_string(),
            task_id: completed.id.clone(),
        },
    );
    apply_turn_event_to_store(
        &store,
        &TurnEvent::TaskFailed {
            task_id: failed.id.clone(),
            error: "boom".to_string(),
        },
    );

    // -- Check
    assert_eq!(
        store
            .get_task(&completed.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Completed
    );
    assert_eq!(
        store
            .get_task(&failed.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Failed
    );
    Ok(())
}

#[test]
fn test_mode_apply_turn_event_to_store_ignores_other_events() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let task = store.create_task(None, None, None);

    // -- Exec
    apply_turn_event_to_store(
        &store,
        &TurnEvent::Started {
            prompt: "hi".to_string(),
            task_id: Some(task.id.clone()),
        },
    );

    // -- Check
    assert_eq!(
        store
            .get_task(&task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Submitted
    );
    Ok(())
}

#[test]
fn test_mode_apply_a2a_turn_result_ok_and_err() -> Result<()> {
    // -- Setup & Fixtures
    let store = InMemoryTaskStore::default();
    let ok_task = working_task(&store)?;
    let err_task = working_task(&store)?;
    let ok: crate::error::Result<Value> = Ok(Value::string("done", Span::unknown()));
    let err: crate::error::Result<Value> = Err(crate::error::Error::Custom("nope".to_string()));

    // -- Exec
    apply_a2a_turn_result(&store, &ok_task.id, &ok);
    apply_a2a_turn_result(&store, &err_task.id, &err);

    // -- Check
    assert_eq!(
        store
            .get_task(&ok_task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Completed
    );
    assert_eq!(
        store
            .get_task(&err_task.id)
            .map_err(|e| format!("{e:?}"))?
            .status
            .state,
        TaskState::Failed
    );
    Ok(())
}

/// A runtime that records the prompts it runs and publishes no turn events.
#[derive(Default)]
struct SingleTurnRuntime {
    prompts: Vec<String>,
}

impl agnt_core::protocol::contracts::CoreRuntime for SingleTurnRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &agnt_core::bus::Bus,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> core::result::Result<Value, nu_protocol::LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(span))
    }
}

#[tokio::test]
async fn test_mode_run_single_turn_publishes_exactly_one_started() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut turn_rx = bus.turn().subscribe();
    let mut runtime = SingleTurnRuntime::default();

    // -- Exec
    agnt_core::orchestrator::run_single_turn(
        &mut runtime,
        &bus,
        "hello".to_string(),
        None,
        Span::unknown(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    // -- Check
    let mut started = Vec::new();
    while let Ok(event) = turn_rx.try_recv() {
        if let TurnEvent::Started { prompt, task_id } = event {
            started.push((prompt, task_id));
        }
    }
    assert_eq!(
        started,
        vec![("hello".to_string(), None)],
        "a single turn must publish exactly one unattributed TurnEvent::Started"
    );
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_start_publishes_started_with_mode() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook(
        "notify",
        &[HookEvent::AgentStarted, HookEvent::AgentShutdown],
    )];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        process.clone(),
    )
    .await;
    let Ok(event) = agent_rx.try_recv() else {
        return Err("the agent channel must carry AgentEvent::Started".into());
    };
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    match event {
        AgentEvent::Started { mode } => assert_eq!(mode, "tui"),
        other => return Err(format!("expected AgentEvent::Started, got {other:?}").into()),
    }
    assert_eq!(drained, 1, "one agent-class hook must spawn one runner");
    let calls = process.calls();
    assert_eq!(calls.len(), 2, "agent_started then agent_shutdown");
    assert_eq!(calls[0].0, "agent_started");
    assert_eq!(calls[1].0, "agent_shutdown");
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_shutdown_publishes_reason() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook("notify", &[HookEvent::AgentShutdown])];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "stderr",
        &hooks,
        std::path::Path::new("/tmp"),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("error").await;
    lifecycle.drain().await;

    // -- Check
    let mut reasons = Vec::new();
    while let Ok(event) = agent_rx.try_recv() {
        if let AgentEvent::Shutdown { reason } = event {
            reasons.push(reason);
        }
    }
    assert_eq!(
        reasons,
        vec!["error".to_string()],
        "the shutdown reason must reach the agent channel"
    );
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "only agent_shutdown is subscribed");
    assert_eq!(calls[0].0, "agent_shutdown");
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_without_agent_hooks_spawns_no_runner() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook("audit", &[HookEvent::TurnCompleted])];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    assert_eq!(
        drained, 0,
        "a hook with no agent-class event must spawn no agent runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[tokio::test]
async fn test_mode_agent_lifecycle_spawns_one_runner_per_agent_hook() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let process = RecordingProcess::default();
    let hooks = vec![
        resolved_hook("one", &[HookEvent::AgentStarted]),
        resolved_hook("two", &[HookEvent::AgentStarted, HookEvent::AgentShutdown]),
    ];

    // -- Exec
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        process.clone(),
    )
    .await;
    lifecycle.shutdown("completed").await;
    let drained = lifecycle.drain().await;

    // -- Check
    assert_eq!(drained, 2, "one runner per agent-class hook");
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        3,
        "hook one: agent_started; hook two: agent_started + agent_shutdown"
    );
    Ok(())
}

#[test]
fn test_mode_shutdown_reason_maps_ok_and_err() -> Result<()> {
    // -- Setup & Fixtures
    let ok: crate::error::Result<Value> = Ok(Value::nothing(Span::unknown()));
    let err: crate::error::Result<Value> = Err(crate::error::Error::Custom("nope".to_string()));

    // -- Exec & Check
    assert_eq!(shutdown_reason(&ok, "completed"), "completed");
    assert_eq!(shutdown_reason(&ok, "shutdown"), "shutdown");
    assert_eq!(shutdown_reason(&err, "completed"), "error");
    assert_eq!(shutdown_reason(&err, "shutdown"), "error");
    Ok(())
}

#[tokio::test]
async fn test_mode_run_with_agent_lifecycle_shuts_down_on_early_body_error() -> Result<()> {
    // -- Setup & Fixtures
    let agent_tx = AgentTx::new("agent", 16);
    let mut agent_rx = agent_tx.subscribe();
    let process = RecordingProcess::default();
    let hooks = vec![resolved_hook(
        "notify",
        &[HookEvent::AgentStarted, HookEvent::AgentShutdown],
    )];
    let lifecycle = AgentLifecycle::start(
        agent_tx,
        "tui",
        &hooks,
        std::path::Path::new("/tmp"),
        process.clone(),
    )
    .await;

    // -- Exec
    let result: crate::error::Result<Value> =
        run_with_agent_lifecycle(lifecycle, "completed", || async {
            Err(crate::error::Error::Custom(
                "renderer init failed".to_string(),
            ))
        })
        .await;

    // -- Check
    assert!(result.is_err(), "the body error must reach the caller");
    let mut reasons = Vec::new();
    while let Ok(event) = agent_rx.try_recv() {
        if let AgentEvent::Shutdown { reason } = event {
            reasons.push(reason);
        }
    }
    assert_eq!(
        reasons,
        vec!["error".to_string()],
        "an early body failure must still publish AgentEvent::Shutdown"
    );
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        2,
        "the hook must observe agent_started and agent_shutdown"
    );
    Ok(())
}

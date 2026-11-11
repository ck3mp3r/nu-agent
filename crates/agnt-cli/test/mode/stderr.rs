//! The stderr mode's peer turn: one turn, attributed to the task it runs for,
//! and one write-back through the turn→terminal mapping.

use super::*;

use agnt_a2a::Part;
use agnt_runtime::conversation::contracts::RuntimeAccess;
use agnt_runtime::conversation::turn::TurnEnd;
use agnt_runtime::orchestrator::external::ExternalTask;
use agnt_runtime::protocol::contracts::CoreRuntime;
use agnt_runtime::session::management::SessionPersistence;
use agnt_tty::policy::{UiPolicy, Verbosity};

/// Every application path, rooted at a directory no test writes to.
fn test_paths() -> agnt_runtime::config::Paths {
    let dir = std::path::PathBuf::from("/tmp/agnt-cli-mode-test");
    agnt_runtime::config::Paths {
        config_path: dir.join("config.toml"),
        dotenv_path: dir.join(".env"),
        agents_dir: dir.join("agents"),
        copilot_token_dir: dir.join("copilot"),
        store_path: dir.join("store.json"),
        log_dir: dir.join("log"),
        log_path: dir.join("log/agent.log"),
        audit_log_path: dir.join("audit.log"),
        secrets_path: dir.join("secrets"),
        models_cache_path: dir.join("models.json"),
        theme_path: dir.join("theme"),
        grammar_cache_dir: dir.join("grammar"),
    }
}

/// A runtime that runs one turn: it records the attribution the dispatch handed
/// it, publishes the turn's own spine bracket the way the executor does, and
/// reports the turn end it is scripted with.
struct PeerTurnRuntime {
    bus: agnt_runtime::bus::Bus,
    paths: agnt_runtime::config::Paths,
    hook_context: agnt_runtime::hooks::HookAgentContext,
    task_ids: Vec<Option<String>>,
    turn_end: Option<TurnEnd>,
}

impl PeerTurnRuntime {
    fn new(bus: agnt_runtime::bus::Bus) -> Self {
        Self {
            bus,
            paths: test_paths(),
            hook_context: agnt_runtime::hooks::HookAgentContext::default(),
            task_ids: Vec::new(),
            turn_end: None,
        }
    }
}

impl CoreRuntime for PeerTurnRuntime {
    async fn execute_turn(
        &mut self,
        prompt: String,
        _context: Option<String>,
        span: Span,
        task_id: Option<String>,
    ) -> TurnEnd {
        self.task_ids.push(task_id.clone());
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnStarted {
            prompt,
            task_id: task_id.clone(),
        });
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id,
            duration_ms: 0,
        });
        self.turn_end.clone().unwrap_or(TurnEnd::Completed {
            value: Value::string("peer response", span),
        })
    }
}

impl RuntimeAccess for PeerTurnRuntime {
    fn paths(&self) -> &agnt_runtime::config::Paths {
        &self.paths
    }

    fn hook_context(&self) -> &agnt_runtime::hooks::HookAgentContext {
        &self.hook_context
    }

    fn bus_handle(&self) -> agnt_runtime::bus::Bus {
        self.bus.clone()
    }

    fn provider_name(&self) -> &str {
        "test"
    }

    fn model(&self) -> &str {
        "test/model"
    }

    fn mcp_caller_cwd(&self) -> Option<&std::path::Path> {
        None
    }

    fn mcp_lifecycle_projection(&self) -> &[agnt_runtime::tools::mcp::runtime::McpServerLifecycle] {
        &[]
    }

    fn available_agent_summaries(&self) -> &[agnt_runtime::protocol::persona::PersonaSummary] {
        &[]
    }

    fn agent_identity(&self) -> Option<&str> {
        None
    }

    fn agent_icon(&self) -> Option<&str> {
        None
    }

    fn startup_plugin_config(&self) -> Option<&agnt_runtime::config::PluginConfig> {
        None
    }

    fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(future)
    }
}

impl SessionPersistence for PeerTurnRuntime {}

/// WHEN stderr mode drains a peer task, THE turn's `TurnStarted`/`TurnCompleted`
/// spine rows SHALL carry that task's id, THE store SHALL transition through the
/// one mapping, and THE mapping SHALL publish one `TaskCompleted` row.
#[tokio::test]
async fn test_mode_run_stderr_mode_peer_turn_carries_the_task_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let mut runtime = PeerTurnRuntime::new(bus);
    let store = std::sync::Arc::new(InMemoryTaskStore::default());
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("{e:?}"))?;
    // The bridged channel is filled by the boundary, so the task is already
    // queued when the mode reaches for it.
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(ExternalTask {
        task_id: task.id.clone(),
        context_id: "ctx-peer".to_string(),
        prompt: "a peer question".to_string(),
    })
    .await
    .map_err(|e| format!("the bridged channel must accept the task: {e}"))?;
    let a2a = A2aContext {
        external_task_rx: Some(rx),
        task_store: Some(std::sync::Arc::clone(&store)),
        ..Default::default()
    };

    // -- Exec
    let value = run_stderr_mode(
        &mut runtime,
        None,
        None,
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        false,
        a2a,
    )
    .await
    .map_err(|e| format!("the stderr peer turn must complete: {e}"))?;

    // -- Check
    assert_eq!(
        value,
        Value::string("peer response", Span::unknown()),
        "the turn's response value is the mode's result"
    );
    assert_eq!(
        runtime.task_ids,
        vec![Some(task.id.clone())],
        "the peer turn must carry the task id the caller held in hand"
    );
    let mut rows = Vec::new();
    while let Ok(event) = lifecycle_rx.try_recv() {
        rows.push(event);
    }
    let attributed = |event: &LifecycleEvent| {
        matches!(
            event,
            LifecycleEvent::TurnStarted { task_id: Some(id), .. }
            | LifecycleEvent::TurnCompleted { task_id: Some(id), .. }
            if id == &task.id
        )
    };
    assert_eq!(
        rows.iter().filter(|event| attributed(event)).count(),
        2,
        "both turn steps must carry the peer task id; got: {rows:?}"
    );
    assert!(
        rows.contains(&LifecycleEvent::TaskCompleted {
            task_id: task.id.clone(),
            output: "peer response".to_string(),
        }),
        "the write-back must publish one task-terminal row; got: {rows:?}"
    );
    let updated = store.get_task(&task.id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(updated.status.state, TaskState::Completed);
    let artifact = updated
        .artifacts
        .iter()
        .find(|artifact| artifact.name.as_deref() == Some("result"))
        .ok_or("the completed task should carry a result artifact")?;
    assert_eq!(
        artifact.parts,
        vec![Part::Text {
            text: "peer response".to_string()
        }],
        "the store records the turn's response text"
    );
    Ok(())
}

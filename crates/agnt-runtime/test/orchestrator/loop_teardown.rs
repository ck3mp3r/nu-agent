//! The loop's teardown when the worker task fails.
//!
//! The worker owns the runtime for the length of the run. A failure there is
//! the one exit that cannot hand a runtime back, so teardown reports it as the
//! loop's outcome instead of panicking mid-close.

use super::*;

// ── PanickingWorkerRuntime ──────────────────────────────────────────────

/// A runtime whose turn panics: the worker task dies with it.
struct PanickingWorkerRuntime;

impl CoreRuntime for PanickingWorkerRuntime {
    async fn execute_turn(
        &mut self,
        _prompt: String,
        _context: Option<String>,
        _span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        panic!("the test runtime panics mid-turn");
    }
}

crate::default_mcp!(PanickingWorkerRuntime);
crate::default_session!(PanickingWorkerRuntime);

impl ModelSwitching for PanickingWorkerRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "unknown/unknown".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

#[tokio::test]
async fn loop_teardown_reports_the_worker_panic_as_the_loop_outcome() -> TResult {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let ui = FakeInteractiveUi::with_prompts(&["panic the worker"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // -- Exec
    let (runtime, result) = drive_loop(
        PanickingWorkerRuntime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;

    // -- Check
    assert!(
        runtime.is_none(),
        "a failed worker leaves no runtime to hand back"
    );
    let error = result.err().ok_or("the failed worker must fail the loop")?;
    assert!(
        error.msg.contains("panicked"),
        "the outcome must name the failed join, got {:?}",
        error.msg
    );
    assert!(
        error.msg.contains("the test runtime panics mid-turn"),
        "the outcome must carry the panic payload, got {:?}",
        error.msg
    );
    Ok(())
}

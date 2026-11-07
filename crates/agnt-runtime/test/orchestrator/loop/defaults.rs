//! Capability defaults: a stage implements one capability and leaves the rest.

use super::*;

use crate::orchestrator::stages::{OrchestrationContext, SlashHandler};

/// A slash stage that records the prompts it handles and implements nothing
/// else: the other three capabilities stay at their no-op defaults.
#[derive(Default)]
struct SlashOnly {
    prompts: Vec<String>,
}

impl SlashHandler for SlashOnly {
    async fn handle(
        &mut self,
        prompt: String,
        _task_id: Option<String>,
        _ctx: &mut OrchestrationContext<'_>,
    ) {
        self.prompts.push(prompt);
    }
}

/// WHEN a stage implements one capability, THE loop SHALL dispatch to it and
/// SHALL dispatch the other three to the default no-op bodies.
#[tokio::test]
async fn one_capability_runs_and_the_others_default_to_noops() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
    let mut slash = SlashOnly::default();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    // -- Exec
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .map_err(|e| format!("the prompt should be sent: {e}"))?;
    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|e| format!("quit should be sent: {e}"))?;

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut (),
            ui_request: &mut (),
            session: &mut (),
        },
        &mut ctx,
    )
    .await;

    // -- Check
    result.map_err(|e| format!("the loop should exit cleanly: {e}"))?;
    assert_eq!(
        slash.prompts,
        vec!["hello".to_string()],
        "the implemented capability must receive the prompt"
    );
    Ok(())
}

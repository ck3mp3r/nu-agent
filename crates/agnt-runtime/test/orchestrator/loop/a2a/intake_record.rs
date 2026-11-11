use super::*;

use crate::orchestrator::stages::session::SessionStage;
use agnt_kernel::SessionRecord;
use agnt_kernel::bus::ControlCommand;

/// WHEN the control arm accepts an external prompt, THE loop SHALL dispatch
/// exactly one turn carrying that task id and SHALL publish no prompt row of
/// its own.
///
/// The row is drawn from the boundary that accepted the arrival — the router,
/// where the runtime's identities are reachable — so the loop owns the admit,
/// the repeat-arrival refusal, and the queueing only. A repeated arrival is not
/// accepted, so it dispatches no second turn.
#[tokio::test]
async fn an_accepted_external_prompt_dispatches_one_attributed_turn() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = h;
    let mut stream_rx = bus.stream().subscribe();
    // The arrival, then the same arrival again: only the first is accepted.
    for _ in 0..2 {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: "task-1".to_string(),
                prompt: "external task".to_string(),
            })
            .map_err(|_| "the control channel should be open")?;
    }
    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = SessionStage;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    // -- Exec: no cancel source is configured, so the surface closing its
    // intake is the loop's exit; the attributed turn is the last thing it does.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "the attributed turn").await? {
            WorkerCommand::ExecuteTurn { task_id, .. } => {
                assert_eq!(task_id.as_deref(), Some("task-1"));
            }
            _ => return Err("expected ExecuteTurn".into()),
        }
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut control_rx,
                &mut lifecycle_rx,
                &mut task_cancel_rx,
            ),
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
        driver
    );
    driven?;
    result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;

    // -- Check: the loop is intake only. The boundary that accepted the
    // arrival publishes the row the surface draws, so the loop publishes none
    // of its own.
    let prompts: Vec<_> = std::iter::from_fn(|| stream_rx.try_recv().ok())
        .filter_map(|record| match record {
            SessionRecord::Prompt(prompt) => Some(prompt),
            _ => None,
        })
        .collect();
    assert!(
        prompts.is_empty(),
        "the accepting boundary publishes the prompt row; the loop publishes none, got {prompts:?}"
    );
    // The repeat arrival was refused, so exactly one turn ran.
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a repeated arrival must not start a second turn"
    );
    Ok(())
}

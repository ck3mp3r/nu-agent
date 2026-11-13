//! Quit gives up on the responses that never arrive.

use super::*;

use std::time::Duration;

use crate::orchestrator::events::UiRequest;
use crate::orchestrator::stages::ui_request::UiRequestStage;

/// A request whose response never arrives holds its slot forever. Quit abandons
/// it rather than waiting for it, on either lane: a blocking switch and a
/// concurrent toggle each defer the quit forever while unanswered.
///
/// Abandoning is not enough on its own: the surface waits on what the fold
/// publishes, so each substitute must be folded before the loop breaks.
#[tokio::test]
async fn quit_abandons_an_unanswered_blocking_request() -> Result<()> {
    // -- Setup & Fixtures
    let rows = [
        (
            "blocking",
            "model switch",
            UiRequest::SwitchModel {
                spec: "wedged-model".to_string(),
            },
        ),
        (
            "concurrent",
            "MCP toggle",
            UiRequest::ToggleMcp {
                server: "wedged-server".to_string(),
                enable: true,
            },
        ),
    ];

    for (label, kind, request) in rows {
        let h = Harness::new();
        let Harness {
            event_tx,
            event_rx,
            worker_tx,
            mut worker_rx,
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
            ..
        } = h;

        let mut stage = UiRequestStage::new(0);
        // The surface reads its release off what the fold publishes: a warning
        // for a switch, a fact row for a toggle.
        let mut narration_rx = bus.narration().subscribe();
        let mut facts_rx = bus.facts().subscribe();
        // The request reaches the worker; the worker never answers.
        event_tx.send(OrchestratorEvent::UiRequest(request)).await?;
        event_tx.send(OrchestratorEvent::Quit).await?;

        let mut slash = MockSlash::new();
        let mut permission = MockPermission::new();
        let mut session = MockSession::new();

        // -- Exec
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let result = tokio::time::timeout(
            Duration::from_secs(2),
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
                    ui_request: &mut stage,
                    session: &mut session,
                },
                &mut ctx,
            ),
        )
        .await;

        // -- Check
        match result {
            Ok(Ok(())) => {}
            _ => {
                return Err(format!(
                    "quit must not wait for the {label} response that never arrives"
                )
                .into());
            }
        }
        assert!(
            matches!(
                recv_command(&mut worker_rx),
                Some(WorkerCommand::HandleUiRequest { .. })
            ),
            "the {label} request reached the worker before quit"
        );
        let mut warnings = Vec::new();
        while let Ok(event) = narration_rx.try_recv() {
            if let agnt_kernel::bus::NarrationEvent::SystemMessage { text, .. } = event {
                warnings.push(text);
            }
        }
        let mut fact_errors = Vec::new();
        while let Ok(fact) = facts_rx.try_recv() {
            if let crate::bus::FactRecord::McpServerState {
                error: Some(error), ..
            } = fact
            {
                fact_errors.push(error);
            }
        }
        let abandoned = format!("{kind} abandoned at quit: the response was not received");
        assert!(
            warnings.contains(&abandoned) || fact_errors.contains(&abandoned),
            "the {label} substitute must be folded before quit breaks, so the surface \
             releases; got warnings: {warnings:?}, facts: {fact_errors:?}"
        );
    }
    Ok(())
}

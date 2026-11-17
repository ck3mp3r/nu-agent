//! Shared fixtures for the dispatch-time skip suite.

use super::*;

use std::sync::Mutex;
use std::time::Duration;

use crate::bus::Bus;
use crate::conversation::turn::TurnEnd;
use crate::orchestrator::external::ExternalTaskStore;
use agnt_kernel::bus::ControlCommand;

/// The idle window the loop has to drain the arrivals the test already
/// published before the wire frees the worker.
pub(super) const INTAKE_SETTLE: Duration = Duration::from_millis(50);

/// The window the wire gives the loop to dispatch the first queued entry once
/// the worker is free.
pub(super) const DISPATCH_SETTLE: Duration = Duration::from_millis(250);

/// A store that answers the state question for the task ids it holds as
/// terminal, and records every rejection the loop records.
///
/// The loop reads the state at dispatch and rejects through the same seam, so
/// one mock answers both calls.
#[derive(Default)]
pub(super) struct StagedStore {
    terminal: Mutex<Vec<String>>,
    rejections: Mutex<Vec<(String, String)>>,
}

impl StagedStore {
    /// A store that reports `task_ids` as terminal and no other task.
    pub(super) fn holding_terminal(task_ids: &[&str]) -> Self {
        Self {
            terminal: Mutex::new(task_ids.iter().map(|id| id.to_string()).collect()),
            ..Self::default()
        }
    }

    /// The `(task_id, reason)` pairs the loop rejected, in order.
    pub(super) fn rejections(&self) -> Vec<(String, String)> {
        self.rejections
            .lock()
            .map(|recorded| recorded.clone())
            .unwrap_or_default()
    }
}

impl ExternalTaskStore for StagedStore {
    fn reject_task(&self, task_id: &str, reason: &str) -> core::result::Result<(), String> {
        self.rejections
            .lock()
            .map_err(|e| e.to_string())?
            .push((task_id.to_string(), reason.to_string()));
        Ok(())
    }

    fn is_task_terminal(&self, task_id: &str) -> bool {
        self.terminal
            .lock()
            .map(|ids| ids.iter().any(|id| id == task_id))
            .unwrap_or(false)
    }
}

/// Publish the arrivals the loop holds in its queue, in arrival order.
pub(super) fn enqueue(bus: &Bus, entries: &[(&str, &str)]) -> Result<()> {
    for (task_id, prompt) in entries {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: task_id.to_string(),
                prompt: prompt.to_string(),
            })
            .map_err(|_| "the control channel should be open")?;
    }
    Ok(())
}

/// Free the busy worker, report the attribution of the first turn the queue then
/// dispatches, complete that turn, and quit the loop.
pub(super) async fn run_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    mut worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
) -> core::result::Result<Option<String>, String> {
    // The arrivals are already on the control intake: freeing the worker before
    // it drained them would dispatch one the test never queued.
    tokio::time::sleep(INTAKE_SETTLE).await;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    // The turn the queue dispatches first, or no turn at all when every queued
    // entry is skipped.
    let first = tokio::time::timeout(DISPATCH_SETTLE, async {
        match worker_rx.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    })
    .await
    .ok()
    .flatten();
    let dispatched = match first {
        Some(WorkerCommand::ExecuteTurn { task_id, .. }) => task_id,
        _ => None,
    };

    if let Some(task_id) = &dispatched {
        let ended = TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        })
        .with_task_id(Some(task_id.clone()));
        worker_result_tx
            .send(ended)
            .await
            .map_err(|_| "the worker result channel should be open".to_string())?;
    }

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(dispatched)
}

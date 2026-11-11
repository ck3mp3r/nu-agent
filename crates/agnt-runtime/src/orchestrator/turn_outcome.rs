use nu_protocol::{LabeledError, Value};

use crate::conversation::turn::TurnEnd;

/// How a turn ended.
///
/// The executor classifies a turn's end as a [`TurnEnd`] — completed, cancelled,
/// or failed with the budget and exhaustion that ended it — and the router maps
/// that variant onto this one. No consumer re-derives a cancellation from an
/// error message.
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// Turn completed successfully with a result value
    Success(Value),
    /// Turn was cancelled by the user
    Cancelled,
    /// Turn failed with an error
    Error(LabeledError),
}

/// One finished turn: how it ended, and the external task whose prompt it ran.
///
/// The task id is set where the turn is dispatched, from the command that
/// carries the attribution, and it travels back with the turn's own result. The
/// consumer that publishes the task's completion therefore reads the id the
/// turn was dispatched under, never a mutable slot that a later dispatch may
/// have overwritten.
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub outcome: TurnOutcome,
    pub task_id: Option<String>,
}

impl TurnResult {
    /// Convert the executor's turn end to a `TurnResult`.
    ///
    /// The end's variant is the classification: a cancelled turn reports
    /// `Cancelled`, and its response value is the single-turn path's, not this
    /// one's. The result carries no attribution: a caller that knows the task
    /// the turn ran for binds it with [`TurnResult::with_task_id`].
    pub fn from_end(end: TurnEnd) -> Self {
        let outcome = match end {
            TurnEnd::Completed { value } => TurnOutcome::Success(value),
            TurnEnd::Cancelled { .. } => TurnOutcome::Cancelled,
            TurnEnd::Failed(failure) => TurnOutcome::Error(failure.error),
        };
        Self {
            outcome,
            task_id: None,
        }
    }

    /// Bind the external task whose prompt this turn ran.
    pub fn with_task_id(mut self, task_id: Option<String>) -> Self {
        self.task_id = task_id;
        self
    }
}

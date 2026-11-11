//! `TurnEnd` — how a single turn ended, as the executor classifies it.
//!
//! The executor knows a turn's end as typed data: the turn produced a response,
//! a stop ended it, or a failure ended it — with the budget that ended it and
//! what the retry loop exhausted, when either applies. `CoreRuntime::execute_turn`
//! returns this value, so a consumer maps the variant it acts on instead of
//! parsing an error message. Message text is content for display, never a
//! signal.

use nu_protocol::{LabeledError, Value};

// region:    --- Types

/// How a turn ended.
#[derive(Debug, Clone)]
pub enum TurnEnd {
    /// The turn produced a response.
    Completed {
        /// The response the surface reports.
        value: Value,
    },

    /// A stop ended the turn before it finished.
    Cancelled {
        /// The response the stop returns.
        value: Value,
    },

    /// The turn failed.
    Failed(TurnFailure),
}

/// Why a turn failed, as the executor classified it.
#[derive(Debug, Clone)]
pub struct TurnFailure {
    /// The user-facing error the surface reports.
    pub error: LabeledError,
    /// The budget that ended the turn, when a budget did.
    pub budget: Option<TurnBudget>,
    /// What the retry/steering loop exhausted before the turn gave up.
    pub exhaustion: Option<TurnExhaustion>,
}

/// The budget that ended a failed turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnBudget {
    /// The configured tool-turn limit ended the turn.
    MaxTurns { limit: usize },
    /// The model's output-token ceiling ended the turn.
    MaxTokens,
}

/// What the retry/steering loop exhausted before the turn failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnExhaustion {
    /// The retry loop failed every attempt. `attempts` counts the retries run
    /// after the first attempt.
    Retries { attempts: usize },
    /// Every steering retry failed and the turn produced no output.
    NoOutput,
}

// endregion: --- Types

// region:    --- TurnFailure

impl TurnFailure {
    /// A failure with no budget and no exhaustion: the turn ended on an error
    /// the executor did not classify further.
    pub fn from_error(error: LabeledError) -> Self {
        Self {
            error,
            budget: None,
            exhaustion: None,
        }
    }

    /// A failure a budget ended.
    pub fn with_budget(error: LabeledError, budget: TurnBudget) -> Self {
        Self {
            error,
            budget: Some(budget),
            exhaustion: None,
        }
    }
}

impl std::fmt::Display for TurnFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error.msg)
    }
}

// endregion: --- TurnFailure

//! The orchestrator family: the loop owner and its phases, the worker router,
//! the stage capabilities, the single-turn entry, and the event vocabulary.

// region:    --- Modules

pub mod config;
pub mod events;
pub mod external;
pub mod handlers;
mod r#loop;
pub mod loop_impl;
pub mod router;
pub mod stages;
pub mod turn_outcome;

pub use router::{CommandRouter, DispatchSources};

pub use config::{AgentSwitchHook, HydrationConfig, LoopConfig, NoAgentSwitchHook};

// Re-export handlers privately for internal use by the loop_impl module.
pub(crate) use handlers::{
    dispatch_compaction, handle_external_cancel, handle_worker_result, recv_or_pending,
    reject_external_task, submit_deferred_prompt,
};
pub(crate) use r#loop::ProductionStages;
pub(crate) use r#loop::SourceChannels;
pub(crate) use r#loop::{CompletionBuffer, ExternalQueue};
pub use r#loop::{Loop, run};
pub use loop_impl::run_single_turn;
#[cfg(test)]
pub(crate) use loop_impl::{Stages, run_orchestrator_loop};

#[cfg(test)]
#[path = "../../test/orchestrator/orchestrator_loop.rs"]
mod orchestrator_loop_test;
#[cfg(test)]
#[path = "../../test/orchestrator/stage.rs"]
mod stage_test;
#[cfg(test)]
#[path = "../../test/orchestrator/test.rs"]
mod test;
#[cfg(test)]
#[path = "../../test/orchestrator/shared.rs"]
mod test_shared;
#[cfg(test)]
#[path = "../../test/orchestrator/turn_outcome.rs"]
mod turn_outcome_test;

// endregion: --- Modules

pub mod config;
pub mod events;
pub mod handlers;
pub mod loop_impl;
pub mod router;
pub mod stages;
pub mod turn_outcome;

pub use router::CommandRouter;

pub use config::{HydrationConfig, InteractiveLoopConfig, OnAgentSwitch};
pub use events::{
    AgentSwitchResult, McpToggleResult, ModelSwitchResult, OrchestratorEvent,
    RefreshSessionPickerResult, SessionSwitchResult, UiRequest, UiRequestResponse, UiStateEvent,
    WorkerCommand,
};

// Re-export handlers privately for internal use by the loop_impl module.
pub(crate) use handlers::{
    dispatch_compaction, handle_external_cancel, handle_external_prompt, handle_worker_result,
    recv_or_pending,
};
#[cfg(test)]
pub(crate) use loop_impl::{
    SourceChannels, Stages, run_interactive_loop_impl, run_orchestrator_loop,
};
pub use loop_impl::{
    run_hydrated_interactive_loop_with_external_prompts,
    run_interactive_loop_with_external_prompts, run_single_turn,
};

#[cfg(test)]
#[path = "orchestrator_loop_test.rs"]
mod orchestrator_loop_test;
#[cfg(test)]
#[path = "stage_test.rs"]
mod stage_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod test_shared;
#[cfg(test)]
mod turn_outcome_test;

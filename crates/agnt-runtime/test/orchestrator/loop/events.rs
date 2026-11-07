//! Orchestrator loop event-handling tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "events/deferred_prompt.rs"]
mod deferred_prompt;
#[path = "events/external_fatal.rs"]
mod external_fatal;
#[path = "events/prompt_routing.rs"]
mod prompt_routing;
#[path = "events/worker_result_quit.rs"]
mod worker_result_quit;

// endregion: --- Modules

use super::*;

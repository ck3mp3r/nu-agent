//! LLM-domain reducer tests: assistant streaming, phase transitions, token
//! accounting, and diff-regurgitation dedup. Assertions moved 1:1 from the
//! former `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests,
//! driven through `LlmState::reduce_llm_event`.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "llm/assistant.rs"]
mod assistant;

#[path = "llm/markdown.rs"]
mod markdown;

#[path = "llm/stopped.rs"]
mod stopped;

#[path = "llm/support.rs"]
mod support;

#[path = "llm/transitions.rs"]
mod transitions;

// The monolith resolved `super::<item>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules

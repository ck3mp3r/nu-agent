//! Hook chain test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "chain/cancel_dispatch.rs"]
mod cancel_dispatch;

#[path = "chain/compaction.rs"]
mod compaction;

#[path = "chain/cost.rs"]
mod cost;

#[path = "chain/deny_dispatch.rs"]
mod deny_dispatch;

#[path = "chain/output_repetition.rs"]
mod output_repetition;

#[path = "chain/permission_display.rs"]
mod permission_display;

#[path = "chain/support.rs"]
mod support;

#[path = "chain/turn_tools.rs"]
mod turn_tools;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::hook::chain`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::bus::Bus;
use crate::compaction::CompactionParams;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::NuCompactor;
use crate::hook::doom_loop::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::{InteractivePermissionResolver, PolicyPermissionResolver};
use crate::protocol::event::PermissionDecision as ProtocolPermissionDecision;
use crate::protocol::event::UiEvent;
use crate::session::CachedMemory;
use crate::tools::handler::builtin_tool::BuiltinTool;
use crate::types::{Message, ToolName};
use futures::StreamExt;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use tempfile::TempDir;

use super::*;

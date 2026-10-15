//! Protocol test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/agents.rs"]
mod agents;

#[path = "test/call_line.rs"]
mod call_line;

#[path = "test/compaction.rs"]
mod compaction;

#[path = "test/json_fields.rs"]
mod json_fields;

#[path = "test/skills.rs"]
mod skills;

#[path = "test/slash.rs"]
mod slash;

#[path = "test/ui_contract.rs"]
mod ui_contract;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::protocol`.
// Sub-files now resolve `super::` against this module root, so the shared
// imports are re-exported here.
use std::fs;

use tempfile::tempdir;

use super::agents::load_agents_chain;
use super::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
    TokenCompactionPolicy,
};
use super::skills::{
    SkillResolveError, SkillSource, discover_skill_catalog, extract_skill_description,
    is_higher_precedence, render_available_skills_preamble_from_catalog,
    resolve_explicit_skill_request,
};
use super::slash::{
    SLASH_COMMAND_ORDER, SlashCommand, SlashParseResult, extract_session_id,
    filter_inline_slash_suggestions, parse_slash_command,
};
use super::tool_args;

use crate::bus::PermissionEvent;
use crate::compaction::CompactionStrategy;
use crate::protocol::event::{PermissionDecision, PermissionRequestContext, UiEvent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

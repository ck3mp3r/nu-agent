//! Shared fixtures for the persona protocol test modules.
//!
//! Holds the imports and the helper functions every topical persona test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::config::AgentsConfig;
pub(super) use crate::protocol::persona::builtins::BUILTIN_PLANNER_CONTENT;
pub(super) use crate::protocol::persona::{
    FrontMatterError, FrontMatterParser, FsPersonaResolver, PersonaError, PersonaFileResolver,
    PersonaLister, PulldownCmarkFrontMatterParser, interpret_front_matter,
};
pub(super) use std::fs;
pub(super) use tempfile::TempDir;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// AgentsConfig with all builtins disabled (for filesystem-only tests)
pub(super) fn no_builtins() -> AgentsConfig {
    AgentsConfig {
        planner_enabled: false,
        maker_enabled: false,
        ..AgentsConfig::default()
    }
}

// endregion: --- Test Support

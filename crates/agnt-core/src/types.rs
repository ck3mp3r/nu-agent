//! Type seam — re-exported from `agnt-runtime` while the runtime layer is
//! strangled out of this crate.
//!
//! Core-internal paths (`crate::types::…`) stay stable during the migration;
//! the type definitions live in `agnt_runtime::types`. The one core-local
//! re-export (the copilot token directory) stays here because it is a config
//! concern, not a shared type.

pub use agnt_runtime::types::*;

// Copilot token directory (config concern — not a shared rig type)
pub use crate::config::copilot_tokens::copilot_token_dir;

//! MCP toggle orchestrator test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the mock
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "mcp_toggle/support.rs"]
mod support;
#[path = "mcp_toggle/tests.rs"]
mod tests;

// endregion: --- Modules

use super::*;
use support::*;

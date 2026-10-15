//! Integration-gated tests for the `agent` CLI.
//!
//! Every test drives the real dispatch functions in-process: no subprocess, no
//! shell, no pipes. External dependencies are stubbed — the LLM is a wiremock
//! server, the XDG tree is a tempdir, and the vault is the file backend rooted
//! in that tempdir.
//!
//! The module is gated behind the `integration` cargo feature, so the default
//! `cargo nextest run` profile never compiles it.

use super::*;

// region:    --- Modules

// This file is loaded through `#[path = "../test/integration.rs"]`, so child
// modules resolve against `test/` rather than `test/integration/`. Every child
// carries an explicit `#[path]` to keep the suite under `test/integration/`.

#[path = "integration/support.rs"]
mod support;

#[path = "integration/a2a.rs"]
mod a2a;
#[path = "integration/agent.rs"]
mod agent;
#[path = "integration/config.rs"]
mod config;
#[path = "integration/main_helpers.rs"]
mod main_helpers;
#[path = "integration/mcp.rs"]
mod mcp;
#[path = "integration/mode.rs"]
mod mode;
#[path = "integration/models.rs"]
mod models;
#[path = "integration/provider.rs"]
mod provider;
#[path = "integration/session.rs"]
mod session;

// endregion: --- Modules

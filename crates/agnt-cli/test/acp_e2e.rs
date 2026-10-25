//! End-to-end ACP stdio tests for the `agent acp` subcommand.
//!
//! Every test launches the `agent` binary as a real subprocess through the ACP
//! SDK client and drives the protocol over stdio. The LLM is a local wiremock
//! server, so no API key and no network access are required.
//!
//! The target is gated behind the `integration` cargo feature through
//! `required-features`, so the default `cargo nextest run` profile never
//! compiles it.

// region:    --- Modules

// This file is the root of a `[[test]]` target, so child modules resolve
// against `test/` rather than `test/acp/`. Every child carries an explicit
// `#[path]` to keep the suite under `test/acp/`.

#[path = "acp/support.rs"]
mod support;

#[path = "acp/config_options.rs"]
mod config_options;
#[path = "acp/hooks.rs"]
mod hooks;
#[path = "acp/permission.rs"]
mod permission;
#[path = "acp/session.rs"]
mod session;

use support::*;

// endregion: --- Modules

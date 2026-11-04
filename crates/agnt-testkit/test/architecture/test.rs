//! Architecture assertion drivers.
//!
//! Each driver loads the workspace, runs one check and compares what the check
//! found to that assertion's recorded baseline. The baselines live in
//! [`support::BASELINES`], one row per assertion; a later checker adds its own
//! module here and its own row there.

// region:    --- Modules

mod graph;
mod identity;
mod report;
mod sources;
mod support;

// endregion: --- Modules

//! Architecture assertion drivers.
//!
//! Each driver loads the workspace, runs one check and compares what the check
//! found to that assertion's recorded baseline. The baselines live in
//! [`support::BASELINES`], one row per assertion; a later checker adds its own
//! module here and its own row there.

// region:    --- Modules

mod baselines_arch6;
mod composition;
mod construction;
mod enums;
mod graph;
mod identity;
mod naming;
mod purity;
mod report;
mod signatures;
mod sources;
mod support;
mod surface;
mod test_doubles;

// endregion: --- Modules

//! Build script for the `agent` CLI.
//!
//! Emits `AGENT_VERSION_MESSAGE`, the string that `agent --version` prints.
//! The message is the cargo package version plus an optional ` (<sha>)`
//! suffix. The build never fails: a missing SHA degrades to the bare version.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AGENT_GIT_SHA");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    println!("cargo:rerun-if-changed=../../.git/packed-refs");

    let version = env!("CARGO_PKG_VERSION");
    let message = match resolve_sha() {
        Some(sha) => format!("{version} ({sha})"),
        None => version.to_string(),
    };
    println!("cargo:rustc-env=AGENT_VERSION_MESSAGE={message}");
}

/// Resolve the short commit SHA: `AGENT_GIT_SHA` first, then `git rev-parse`.
///
/// Returns `None` when neither source yields a value, so the caller prints the
/// bare version. Nix builds inject `AGENT_GIT_SHA` because the store source has
/// no `.git` directory.
fn resolve_sha() -> Option<String> {
    if let Ok(sha) = std::env::var("AGENT_GIT_SHA")
        && !sha.is_empty()
    {
        return Some(sha);
    }

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let full = String::from_utf8(output.stdout).ok()?;
    full.trim().get(..8).map(String::from)
}

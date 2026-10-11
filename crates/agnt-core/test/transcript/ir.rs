//! Transcript IR test suite: Block, BlockSource, ContentKind, Display,
//! DisplaySection, Fill, and the projection pipeline.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the `projected_lines_text` helper live here; each sub-file pulls
//! them in with `use super::*;`.

use super::ir::*;
use super::items::{Banner, Message, Spacer};
use super::renderer::{ItemStatus, Renderable};
use crate::protocol::tool_args::CallLine;

// region:    --- Modules

#[path = "ir/block.rs"]
mod block;
#[path = "ir/content.rs"]
mod content;
#[path = "ir/display.rs"]
mod display;
#[path = "ir/fill.rs"]
mod fill;
#[path = "ir/project.rs"]
mod project;
#[path = "ir/project_notice.rs"]
mod project_notice;
#[path = "ir/project_suppression.rs"]
mod project_suppression;
#[path = "ir/renderable.rs"]
mod renderable;

// endregion: --- Modules

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Flatten projected lines to per-line plain text for whole-line assertions.
/// Whole-line checks avoid false matches against the call-summary substring
/// ("→ foo.rs (diff)").
fn projected_lines_text(lines: &[ContentLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect()
}

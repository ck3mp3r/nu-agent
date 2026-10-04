//! Workspace-wide architecture invariants for the Block rendering model
//! (task 7eb16881). Each test enforces one grep criterion from the task spec
//! so regressions fail the test suite instead of waiting for a manual grep.
//!
//! The patterns run over production AND test sources — the architectural
//! rules hold at both layers.

use crate::test_support::{first_file_containing, workspace_rs_files};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- String sniffing bans

/// `name == "nu"` string sniffing is banned: tool identity flows through
/// `BuiltinKind`/`ToolName`, never through string comparison.
#[test]
fn workspace_has_zero_name_equals_nu_sniffing() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "name == \"nu\"");
    assert!(
        hit.is_none(),
        "string sniffing `name == \"nu\"` found in {hit:?} — use typed tool identity"
    );
}

/// `language == "diff"` string sniffing is banned: content kind flows through
/// `ContentKind::Diff { .. }` pattern matches, never through string compare.
#[test]
fn workspace_has_zero_language_equals_diff_sniffing() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "language == \"diff\"");
    assert!(
        hit.is_none(),
        "string sniffing `language == \"diff\"` found in {hit:?} — match ContentKind::Diff {{ .. }}"
    );
}

/// `starts_with("edit ")` string sniffing is banned: display semantics flow
/// through typed tool identity (`BuiltinKind`), not title-text prefix probes.
#[test]
fn workspace_has_zero_edit_prefix_sniffing() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "starts_with(\"edit \")");
    assert!(
        hit.is_none(),
        "string sniffing `starts_with(\"edit \")` found in {hit:?} — use BuiltinKind"
    );
}

// endregion: --- String sniffing bans

// region:    --- Dead-code bans (old rendering model)

#[test]
fn workspace_has_zero_block_renderer_references() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "BlockRenderer");
    assert!(hit.is_none(), "old `BlockRenderer` found in {hit:?}");
}

#[test]
fn workspace_has_zero_to_render_block_references() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "to_render_block");
    assert!(hit.is_none(), "old `to_render_block` found in {hit:?}");
}

#[test]
fn workspace_has_zero_transcript_entry_kind_references() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "TranscriptEntryKind");
    assert!(hit.is_none(), "old `TranscriptEntryKind` found in {hit:?}");
}

#[test]
fn workspace_has_zero_call_line_render_references() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "CallLineRender");
    assert!(hit.is_none(), "old `CallLineRender` found in {hit:?}");
}

/// Batch of removed helper symbols from earlier subtasks; one test because
/// they share the same failure surface (a stale reference anywhere).
#[test]
fn workspace_has_zero_removed_helper_symbols() {
    let files = workspace_rs_files();
    for pattern in [
        "code_block_line_flags",
        "row_needs_user_bg",
        "spacer_count",
        "renders_background_block",
        "is_tool_entry",
        "recompute_entry_visual_info",
        "push_block_spacers",
    ] {
        let hit = first_file_containing(&files, pattern);
        assert!(
            hit.is_none(),
            "removed helper symbol `{pattern}` found in {hit:?}"
        );
    }
}

// endregion: --- Dead-code bans (old rendering model)

// region:    --- Store API bans

/// The old push APIs are deleted; `push_block()` is the sole push path.
/// Function-DEFINITION greps are scoped to `transcript_store.rs` (per the
/// task criteria; test-module helpers share the prefix by coincidence),
/// while CALL-SITE greps run workspace-wide.
#[test]
fn workspace_has_zero_old_push_apis() -> Result<()> {
    let files = workspace_rs_files();
    let store_path = files
        .iter()
        .find(|p| p.ends_with("state/transcript_store.rs"))
        .ok_or("transcript_store.rs must exist under crates/")?;
    let store = std::fs::read_to_string(store_path)?;
    for pattern in [
        "fn push_spacer",
        "fn push_transcript_line",
        "fn push_tool_display_lines",
    ] {
        assert!(
            !store.contains(pattern),
            "banned old push API definition `{pattern}` found in transcript_store.rs"
        );
    }
    for pattern in [
        "push_spacer()",
        "push_transcript_line(",
        "push_tool_display_lines(",
    ] {
        let hit = first_file_containing(&files, pattern);
        assert!(
            hit.is_none(),
            "banned old push API call `{pattern}` found in {hit:?}"
        );
    }
    Ok(())
}

/// No free constructor functions (`fn name_block(`, `fn name_block_for(`) and
/// no free function returning `Block` in the transcript store — the Renderable
/// trait is the sole construction path and callers build Block directly.
/// `push_block` is the one mandated `_block`-named function and is exempt.
#[test]
fn transcript_store_has_zero_free_block_constructors() -> Result<()> {
    let files = workspace_rs_files();
    let store_path = files
        .iter()
        .find(|p| p.ends_with("state/transcript_store.rs"))
        .ok_or("transcript_store.rs must exist under crates/")?;
    let source = std::fs::read_to_string(store_path)?;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("fn ") && !trimmed.starts_with("pub") {
            continue;
        }
        let Some(paren_idx) = trimmed.find('(') else {
            continue;
        };
        let before = &trimmed[..paren_idx];
        let has_block_return = before.contains("-> Block") || before.trim_end().ends_with("Block");
        let name = before.rsplit(' ').next().unwrap_or(before);
        if name == "push_block" {
            assert!(
                !has_block_return,
                "push_block must push, not return, a Block: {trimmed}"
            );
            continue;
        }
        assert!(
            !name.ends_with("_block") && !name.ends_with("_block_for"),
            "free block constructor `{name}` found in transcript_store.rs — construct Block directly via Renderable methods"
        );
        assert!(
            !has_block_return,
            "free function returning Block found in transcript_store.rs: {trimmed}"
        );
    }
    Ok(())
}

// endregion: --- Store API bans

// region:    --- Projection ownership

/// Projection is an inherent method on `BlockSource` (`project`), not a free
/// function on the renderer. The free function must not exist, and the method
/// must be reachable (the `impl` is in core where the projection helpers and
/// `ContentLine` live — an inherent impl on a foreign type is illegal by the
/// orphan rule, so it cannot live in nu-agent-tui).
#[test]
fn workspace_has_no_project_source_free_function() {
    let files = workspace_rs_files();
    let hit = first_file_containing(&files, "fn project_source");
    assert!(
        hit.is_none(),
        "free function `project_source` found in {hit:?} — projection is BlockSource::project"
    );
}

/// `BlockSource::project` is defined exactly once (ir.rs).
#[test]
fn block_source_project_is_defined_once() {
    let files = workspace_rs_files();
    let defs: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let content = std::fs::read_to_string(p).ok()?;
            if content.contains("fn project(&self, width: usize)") {
                Some(p.display().to_string())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        defs.len(),
        1,
        "`BlockSource::project` must be defined exactly once; found: {defs:?}"
    );
}

// endregion: --- Projection ownership

// region:    --- Lane owns styling

/// `LaneContext::from` derives styling from `block.lane` alone — it must never
/// inspect `block.source`. Lane styling is decided at construction time
/// (`Renderable::lane`), so a source match here is a layering violation
/// (task 46ca79fe). `render_block` no longer inspects `block.source` for
/// layout either: banner centering reads `block.lane == Lane::SystemBlank`
/// (task 74ad8b1b), so this test's scope covers the only remaining source
/// match site.
#[test]
fn lane_context_from_does_not_inspect_block_source() -> Result<()> {
    let files = workspace_rs_files();
    let renderer_path = files
        .iter()
        .find(|p| p.ends_with("src/tui_renderer.rs"))
        .ok_or("tui_renderer.rs must exist under crates/")?;
    let source = std::fs::read_to_string(renderer_path)?;
    let fn_idx = source
        .find("fn from(block: &Block, theme: &TuiTheme) -> Self")
        .ok_or("LaneContext::from must exist")?;
    let body = &source[fn_idx..];
    // The `from` body ends at the impl block's closing brace; search the
    // first 2000 bytes, which covers the full match arms.
    let window = &body[..body.len().min(2000)];
    assert!(
        !window.contains("block.source"),
        "LaneContext::from must not inspect block.source — lane styling comes from block.lane"
    );
    Ok(())
}

// endregion: --- Lane owns styling

// region:    --- Style mapping dedup

/// `hint_to_style` must be defined exactly once in the TUI crate. Today it
/// exists twice (tui_renderer.rs lane-aware copy + runtime/status/help.rs
/// theme-only copy) — the duplication task 7eb16881 removes.
#[test]
fn tui_crate_defines_hint_to_style_exactly_once() {
    let files = workspace_rs_files();
    let defs: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let content = std::fs::read_to_string(p).ok()?;
            let count = content.matches("fn hint_to_style").count();
            if count > 0 {
                Some(format!("{} (x{count})", p.display()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        defs.len(),
        1,
        "hint_to_style must be defined exactly once in the TUI crate; found: {defs:?}"
    );
}

// endregion: --- Style mapping dedup

// region:    --- Status owns its indicator

/// `indicator_char` is an inherent method on `ItemStatus`, not a free function.
/// Every definition of `indicator_char` must carry a receiver (`&self`); a
/// definition without one — at any indentation — is the SRP violation this
/// invariant prevents from returning. The method's own signature contains the
/// same `fn indicator_char` substring, so the check keys on the receiver.
#[test]
fn workspace_has_no_indicator_char_free_function() {
    let files = workspace_rs_files();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            if line.contains("fn indicator_char") && !line.contains("self") {
                panic!(
                    "free function `indicator_char` found in {}: {} — the indicator is ItemStatus::indicator_char",
                    path.display(),
                    line.trim()
                );
            }
        }
    }
}

/// `ItemStatus::indicator_char` is defined exactly once (renderer.rs).
#[test]
fn item_status_indicator_char_is_defined_once() {
    let files = workspace_rs_files();
    let defs: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let content = std::fs::read_to_string(p).ok()?;
            if content.contains("fn indicator_char(&self, now_millis: u128)") {
                Some(p.display().to_string())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        defs.len(),
        1,
        "`ItemStatus::indicator_char` must be defined exactly once; found: {defs:?}"
    );
}

// endregion: --- Status owns its indicator

//! Architecture ban: the event fold is the single writer.
//!
//! Design note `6ad10855` §4.4 and decision 14: the event-fold state — the
//! transcript store, the permission prompts, the prompt queue, the phase, the
//! input lock, and the streaming bookkeeping — is written only inside
//! `reduce`. The input-domain lenses keep their own state (pending keys, panel
//! scroll, picker state); that is out of this ban's scope.
//!
//! The checks are cheap source greps with regression value: one module
//! dispatches `SystemEvent`, and the fold's domain writers are called nowhere
//! else. The source-shape bans (no test-only hooks, no sniffing, one call-key
//! derivation) live in [`source_shape`].

// region:    --- Modules

mod source_shape;

// endregion: --- Modules

use std::path::{Path, PathBuf};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The only module allowed to fold bus events into state.
const REDUCER: &str = "interaction/reducer.rs";

/// The TUI's widget modules, relative to `src`.
const WIDGET_SURFACE: &str = "widgets";

/// The backend-neutral render crate's sources, relative to this crate's root.
const RENDER_CRATE: &str = "../agnt-render/src";

/// The terminal library the render crate must never name.
const TERMINAL_LIBRARY: &str = "ratatui";

/// The syntax that introduces a trait object.
const DYNAMIC_DISPATCH: &str = "dyn ";

/// Calls that mutate the event-fold state. They belong to the reducer's arms.
const FOLD_WRITERS: [&str; 17] = [
    ".llm.started(",
    ".llm.chunk(",
    ".llm.completed(",
    ".llm.stopped(",
    ".tool.start_call(",
    ".tool.complete_call(",
    ".tool.set_tool_preview(",
    ".tool.set_preview_for_key(",
    ".permission.open_prompt(",
    ".permission.submit_decision(",
    ".permission.close_prompt(",
    ".permission.close_prompt_if(",
    ".compaction.started(",
    ".compaction.summary_chunk(",
    ".compaction.completed(",
    ".compaction.failed(",
    ".turn.finalize(",
];

#[test]
fn test_architecture_single_writer_fold_state_is_written_only_in_reduce() -> Result<()> {
    // -- Setup & Fixtures
    let sources = module_sources()?;

    // -- Exec
    let violations: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| fold_writer_violations(module, source))
        .collect();

    // -- Check
    assert!(
        violations.is_empty(),
        "event-fold state is written outside `reduce`: {violations:#?}"
    );
    Ok(())
}

#[test]
fn test_architecture_single_writer_events_are_dispatched_only_in_reduce() -> Result<()> {
    // -- Setup & Fixtures
    let sources = module_sources()?;

    // -- Exec
    let violations: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| dispatcher_violations(module, source))
        .collect();

    // -- Check
    assert!(
        violations.is_empty(),
        "`SystemEvent` is matched outside `reduce`: {violations:#?}"
    );
    Ok(())
}

#[test]
fn test_architecture_single_writer_check_flags_a_second_writer() -> Result<()> {
    // -- Setup & Fixtures: a module that folds an event and writes the state.
    let rogue = "runtime/rogue.rs";
    let source = concat!(
        "fn rogue(state: &mut AppState, event: SystemEvent) -> bool {\n",
        "    state.llm.started();\n",
        "    match event {\n",
        "        SystemEvent::Tick => true,\n",
        "    }\n",
        "}\n",
    );

    // -- Exec & Check: the rogue module is flagged by both checks.
    assert_eq!(fold_writer_violations(rogue, source).len(), 1);
    assert_eq!(dispatcher_violations(rogue, source).len(), 1);

    // -- Exec & Check: the same source is legal inside the reducer.
    assert!(fold_writer_violations(REDUCER, source).is_empty());
    assert!(dispatcher_violations(REDUCER, source).is_empty());
    Ok(())
}

// region:    --- Render surface bans

/// WHEN the render surface is scanned, THE widgets and the render crate SHALL
/// carry no trait objects: heterogeneous display is the entry enum plus a
/// match, never a trait-object container (design `6ad10855` §3.4, decision 8).
#[test]
fn test_architecture_render_surface_bans_dynamic_dispatch() -> Result<()> {
    // -- Setup & Fixtures
    let sources = render_surface_sources()?;

    // -- Exec
    let violations: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| dynamic_dispatch_violations(module, source))
        .collect();

    // -- Check
    assert!(
        sources
            .iter()
            .any(|(module, _)| module.starts_with(&format!("{WIDGET_SURFACE}/"))),
        "the ban must scan the widget modules"
    );
    assert!(
        violations.is_empty(),
        "the render surface uses dynamic dispatch: {violations:#?}"
    );
    Ok(())
}

/// WHEN the ban check runs on a rogue widget, THE check SHALL flag it, and THE
/// same code inside a generic bound SHALL pass.
#[test]
fn test_architecture_dynamic_dispatch_ban_flags_a_rogue_widget() -> Result<()> {
    // -- Setup & Fixtures: a widget painted through a trait object.
    let rogue = "widgets/rogue.rs";
    let source = concat!(
        "fn rogue(widget: &dyn PaneWidget) {\n",
        "    widget.paint();\n",
        "}\n",
    );

    // -- Exec & Check: the trait object is flagged.
    assert_eq!(dynamic_dispatch_violations(rogue, source).len(), 1);

    // -- Exec & Check: the same dispatch through a generic bound is legal.
    let legal = "fn paint<W: PaneWidget>(widget: &W) {}\n";
    assert!(dynamic_dispatch_violations(rogue, legal).is_empty());
    Ok(())
}

/// WHEN the render crate is scanned, THE terminal library SHALL not appear in
/// any of its sources: the catalog contract is backend-neutral.
#[test]
fn test_architecture_render_crate_is_terminal_library_free() -> Result<()> {
    // -- Setup & Fixtures
    let sources = render_crate_sources()?;

    // -- Exec
    let violations: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| terminal_library_violations(module, source))
        .collect();

    // -- Check
    assert!(
        violations.is_empty(),
        "the render crate names a terminal library: {violations:#?}"
    );
    Ok(())
}

/// WHEN the terminal-library check runs on an import, THE check SHALL flag it.
#[test]
fn test_architecture_terminal_library_ban_flags_an_import() -> Result<()> {
    // -- Setup & Fixtures
    let rogue = "agnt-render/src/rogue.rs";
    let source = "use ratatui::buffer::Buffer;\n";

    // -- Exec & Check
    assert_eq!(terminal_library_violations(rogue, source).len(), 1);
    assert!(
        terminal_library_violations(rogue, "use agnt_core::transcript::ir::Span;\n").is_empty()
    );
    Ok(())
}

// endregion: --- Render surface bans

// region:    --- Test Support

/// Every production module of the crate, as (path relative to `src`, source).
fn module_sources() -> Result<Vec<(String, String)>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files)?;
    files
        .into_iter()
        .map(|file| {
            let module = file
                .strip_prefix(&root)?
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&file)?;
            Ok((module, source))
        })
        .collect()
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The render surface sources: the TUI widget modules plus the render crate,
/// labelled crate-relative.
fn render_surface_sources() -> Result<Vec<(String, String)>> {
    let widget_prefix = format!("{WIDGET_SURFACE}/");
    let mut sources: Vec<(String, String)> = module_sources()?
        .into_iter()
        .filter(|(module, _)| module.starts_with(&widget_prefix))
        .collect();
    sources.extend(render_crate_sources()?);
    Ok(sources)
}

/// The backend-neutral render crate's sources, labelled by crate.
fn render_crate_sources() -> Result<Vec<(String, String)>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    sources_under(
        &manifest.join(RENDER_CRATE),
        &manifest.join("../agnt-render"),
    )
}

/// Every Rust source under `dir`, labelled relative to `base`.
fn sources_under(dir: &Path, base: &Path) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files)?;
    files
        .into_iter()
        .map(|file| {
            let module = file
                .strip_prefix(base)?
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&file)?;
            Ok((module, source))
        })
        .collect()
}

fn dynamic_dispatch_violations(module: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(DYNAMIC_DISPATCH))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

fn terminal_library_violations(module: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(TERMINAL_LIBRARY))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

fn fold_writer_violations(module: &str, source: &str) -> Vec<String> {
    if module == REDUCER {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| FOLD_WRITERS.iter().any(|writer| line.contains(writer)))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

fn dispatcher_violations(module: &str, source: &str) -> Vec<String> {
    if module == REDUCER {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| is_event_pattern(line))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

/// A match pattern over the vocabulary: construction always sits inside a call
/// or a `let`, so a line that begins with the variant is a fold arm.
fn is_event_pattern(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("SystemEvent::") || line.starts_with("| SystemEvent::")
}

// endregion: --- Test Support

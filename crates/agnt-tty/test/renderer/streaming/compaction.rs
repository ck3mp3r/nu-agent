//! Compaction lines: the success color and the plain no-color form.

use super::*;

#[test]
fn compaction_completed_uses_success_color() -> core::result::Result<(), Box<dyn std::error::Error>>
{
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );
    draw(
        &mut renderer,
        SessionRecord::Compaction(CompactionRecord::Completed {
            source: Some(CompactionSource::Auto),
            summary: "s".to_string(),
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("\u{1b}[32m"));
    Ok(())
}

#[test]
fn uncompacted_renders_the_undone_status() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    draw(
        &mut renderer,
        SessionRecord::Compaction(CompactionRecord::Uncompacted {
            source: CompactionSource::Manual,
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(
        stderr_out.contains("compaction: source=slash status=undone"),
        "the undo row must name the status, got: {stderr_out}"
    );
    Ok(())
}

#[test]
fn no_color_renders_compaction_plain() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    draw(
        &mut renderer,
        SessionRecord::Compaction(CompactionRecord::Completed {
            source: Some(CompactionSource::Auto),
            summary: "s".to_string(),
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(!stderr_out.contains("\u{1b}["));
    Ok(())
}

#[test]
fn compaction_completed_without_source_drops_the_source_token()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures: a replayed completion, which names no source.
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    // -- Exec
    draw(
        &mut renderer,
        SessionRecord::Compaction(CompactionRecord::Completed {
            source: None,
            summary: "the parser rewrite".to_string(),
        }),
    );
    renderer.flush();

    // -- Check: the row keeps its preview and shows no source token.
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(
        stderr_out.contains("compaction: preview=the parser rewrite"),
        "the row carries the summary preview: {stderr_out:?}"
    );
    assert!(
        !stderr_out.contains("source="),
        "a record that names no source shows no source token: {stderr_out:?}"
    );
    Ok(())
}

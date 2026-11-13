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
            source: CompactionSource::Auto,
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
            source: CompactionSource::Auto,
            summary: "s".to_string(),
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(!stderr_out.contains("\u{1b}["));
    Ok(())
}

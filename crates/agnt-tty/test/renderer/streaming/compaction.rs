//! Compaction lines: the success color and the plain no-color form.

use super::*;

#[test]
fn compaction_completed_uses_success_color() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );
    renderer.emit(&UiEvent::CompactionCompleted {
        source: "auto".to_string(),
        summary_preview: "s".to_string(),
        summary_body: "s".to_string(),
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("\u{1b}[32m"));
}

#[test]
fn no_color_renders_compaction_plain() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    renderer.emit(&UiEvent::CompactionCompleted {
        source: "auto".to_string(),
        summary_preview: "s".to_string(),
        summary_body: "s".to_string(),
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("\u{1b}["));
}

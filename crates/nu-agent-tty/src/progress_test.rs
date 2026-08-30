use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nu_agent_core::protocol::contracts::ProgressUi;

use crate::policy::{UiPolicy, Verbosity};
use crate::progress::StderrProgressUi;
use crate::renderer::StderrUiRenderer;

#[test]
fn stderr_progress_ui_take_cancel_requested_returns_flag_and_clears() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut stderr_bytes = Vec::<u8>::new();
    let renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    let ui = StderrProgressUi::new(renderer, Arc::clone(&flag));

    // Initially false
    assert!(!ui.take_cancel_requested());

    // Set the flag
    flag.store(true, Ordering::SeqCst);

    // Should return true and clear
    assert!(ui.take_cancel_requested());

    // Should now be cleared
    assert!(!ui.take_cancel_requested());
}

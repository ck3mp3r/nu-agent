use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::renderer::UiRenderer;
use agnt_runtime::protocol::contracts::ProgressUi;
use agnt_runtime::protocol::events::SystemEvent;

pub struct StderrProgressUi<R>
where
    R: UiRenderer,
{
    renderer: R,
    cancel_requested: Arc<AtomicBool>,
}

impl<R> StderrProgressUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: R, cancel_requested: Arc<AtomicBool>) -> Self {
        Self {
            renderer,
            cancel_requested,
        }
    }
}

impl<R> ProgressUi for StderrProgressUi<R>
where
    R: UiRenderer,
{
    fn emit(&mut self, event: &SystemEvent) {
        self.renderer.emit(event);
    }

    fn flush(&mut self) {
        self.renderer.flush();
    }

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }
}

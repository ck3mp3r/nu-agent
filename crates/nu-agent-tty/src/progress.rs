use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nu_agent_core::protocol::contracts::ProgressUi;
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;

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
    fn emit(&mut self, event: &UiEvent) {
        self.renderer.emit(event);
    }

    fn flush(&mut self) {
        self.renderer.flush();
    }

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }
}

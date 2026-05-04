use nu_protocol::{Span, Value};

use super::{
    contracts::{InteractiveUi, ProgressUi, UiMessageSnapshot},
    ui::{
        event::UiEvent,
        renderer::UiRenderer,
        tui::runtime::{HybridTerminalEvents, TuiRuntimeRenderer},
    },
};

pub(crate) struct StderrProgressUi<R>
where
    R: UiRenderer,
{
    renderer: R,
}

impl<R> StderrProgressUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: R) -> Self {
        Self { renderer }
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
        false
    }
}

pub(crate) struct TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>,
}

impl<R> TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>) -> Self {
        Self { renderer }
    }

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.renderer
            .set_active_model_identity(active_model_identity);
    }
}

impl<R> ProgressUi for TuiInteractiveUi<R>
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
        self.renderer.take_cancel_requested()
    }

    fn cancellation_value(&self, span: Span) -> Option<Value> {
        Some(Value::nothing(span))
    }
}

impl<R> InteractiveUi for TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    fn pump_once(&mut self) {
        self.renderer.pump_terminal_once();
    }

    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.renderer.take_submitted_prompt()
    }

    fn quit_requested(&self) -> bool {
        self.renderer.quit_requested()
    }

    fn fatal_error(&self) -> Option<&str> {
        self.renderer.fatal_error()
    }

    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        self.renderer.hydrate_transcript_from_messages(messages);
    }
}

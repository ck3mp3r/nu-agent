use nu_protocol::{LabeledError, Span, Value};

use super::ui::event::UiEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiMessageSnapshot {
    role: String,
    content: String,
}

impl UiMessageSnapshot {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

pub(crate) trait ProgressUi {
    fn emit(&mut self, event: &UiEvent);
    fn flush(&mut self);
    fn take_cancel_requested(&self) -> bool;
    fn cancellation_value(&self, _span: Span) -> Option<Value> {
        None
    }
}

pub(crate) trait InteractiveUi: ProgressUi {
    fn pump_once(&mut self);
    fn take_submitted_prompt(&mut self) -> Option<String>;
    fn quit_requested(&self) -> bool;
    fn fatal_error(&self) -> Option<&str>;
    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    );
}

pub(crate) trait ConversationRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError>;
}

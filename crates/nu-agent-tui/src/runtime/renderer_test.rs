use super::*;

impl<R: UiRenderer, E: TerminalEventSource> TuiRuntimeRenderer<R, E> {
    pub(crate) fn new_tui_active_for_test(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
    ) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, true)
    }

    pub(crate) fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }
}

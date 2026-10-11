use crate::{
    interaction::input::{TerminalEvent, TerminalKey},
    runtime::{HybridTerminalEvents, TerminalEventSource},
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct SequenceEventSource {
    events: Vec<core::result::Result<Option<TerminalEvent>, String>>,
    idx: usize,
}

impl SequenceEventSource {
    fn new(events: Vec<core::result::Result<Option<TerminalEvent>, String>>) -> Self {
        Self { events, idx: 0 }
    }
}

impl TerminalEventSource for SequenceEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        if self.idx >= self.events.len() {
            return Ok(None);
        }

        let event = self.events[self.idx].clone();
        self.idx += 1;
        event
    }
}

#[test]
fn fallback_event_is_used_when_primary_is_idle() {
    let primary = SequenceEventSource::new(vec![Ok(None)]);
    let fallback =
        SequenceEventSource::new(vec![Ok(Some(TerminalEvent::Key(TerminalKey::Char('x'))))]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    let event = hybrid.poll_event();
    assert_eq!(event, Ok(Some(TerminalEvent::Key(TerminalKey::Char('x')))));
}

#[test]
fn fallback_event_is_used_when_primary_errors() {
    let primary = SequenceEventSource::new(vec![Err("primary failed".to_string())]);
    let fallback =
        SequenceEventSource::new(vec![Ok(Some(TerminalEvent::Key(TerminalKey::Char('y'))))]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    let event = hybrid.poll_event();
    assert_eq!(event, Ok(Some(TerminalEvent::Key(TerminalKey::Char('y')))));
}

#[test]
fn combined_error_reports_primary_and_fallback_failure() {
    let primary = SequenceEventSource::new(vec![Err("primary failed".to_string())]);
    let fallback = SequenceEventSource::new(vec![Err("fallback failed".to_string())]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    let error = hybrid.poll_event().expect_err("expected combined error");
    assert!(error.contains("primary failed"));
    assert!(error.contains("fallback failed"));
}

#[test]
fn diagnostics_report_fallback_backend_and_last_poll_state() {
    let primary = SequenceEventSource::new(vec![Ok(None)]);
    let fallback = SequenceEventSource::new(vec![Ok(Some(TerminalEvent::Key(TerminalKey::Enter)))]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    let event = hybrid.poll_event();
    assert_eq!(event, Ok(Some(TerminalEvent::Key(TerminalKey::Enter))));

    let diagnostics = hybrid.diagnostics();
    assert_eq!(diagnostics.active_backend, "tty");
    assert_eq!(diagnostics.last_poll_state, "/dev/tty delivered event");
    assert_eq!(diagnostics.fallback_available, Some(true));
}

#[test]
fn diagnostics_mark_both_backends_unavailable_on_double_failure() -> Result<()> {
    let primary = SequenceEventSource::new(vec![Err("primary failed".to_string())]);
    let fallback = SequenceEventSource::new(vec![Err("fallback failed".to_string())]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    let _ = hybrid.poll_event();
    let diagnostics = hybrid.diagnostics();
    assert_eq!(diagnostics.primary_available, Some(false));
    assert_eq!(diagnostics.fallback_available, Some(false));
    assert_eq!(diagnostics.active_backend, "none");
    let last_error = diagnostics.last_error.ok_or("should have last error")?;
    assert!(last_error.contains("fallback failed"));
    Ok(())
}

#[test]
fn fallback_backend_emits_char_enter_and_ctrlc_when_primary_is_idle() {
    let primary = SequenceEventSource::new(vec![Ok(None), Ok(None), Ok(None)]);
    let fallback = SequenceEventSource::new(vec![
        Ok(Some(TerminalEvent::Key(TerminalKey::Char('a')))),
        Ok(Some(TerminalEvent::Key(TerminalKey::Enter))),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlC))),
    ]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    assert_eq!(
        hybrid.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Char('a'))))
    );
    assert_eq!(
        hybrid.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Enter)))
    );
    assert_eq!(
        hybrid.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlC)))
    );
}

#[test]
fn fallback_backend_emits_ctrlp_when_primary_is_idle() {
    let primary = SequenceEventSource::new(vec![Ok(None)]);
    let fallback = SequenceEventSource::new(vec![Ok(Some(TerminalEvent::Key(TerminalKey::CtrlP)))]);
    let mut hybrid = HybridTerminalEvents::new_for_test(primary, fallback);

    assert_eq!(
        hybrid.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlP)))
    );
}

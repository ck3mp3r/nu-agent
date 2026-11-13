//! The slash round-trip composition, pinned through the real render loop.
//!
//! The halves live in different crates: the surface hands a prompt to the
//! orchestrator channel, and the effect records come back on the bus. This file
//! composes them on one harness — a driver running `run_render_loop` against a
//! real `Bus` — so a break in the seam between the halves fails here, not only
//! in the crate that owns one half.

use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};
use crate::state::UiPhase;
use crate::test_support::{session_started_resumed, system_message};
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::events::NoticeKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN the user types `/new` and submits it, THE render loop SHALL hand exactly
/// one `/new` prompt to the orchestrator, place no user echo row, and end idle.
#[tokio::test]
async fn test_runtime_slash_round_trip_typed_new_hands_off() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type the command and submit it, key by key.
    driver.advance(&typed("/new")).await?;

    // -- Check: one handoff, no echo row, idle.
    assert_eq!(
        submitted_texts(&driver),
        vec!["/new"],
        "the loop must hand the command off exactly once"
    );
    assert!(
        driver.state().transcript.entries().is_empty(),
        "a slash command appends no user echo row"
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    Ok(())
}

/// WHEN the user types `/`, narrows the inline picker to `/new`, and accepts it
/// with Enter, THE render loop SHALL hand off the same `/new` prompt the typed
/// lane produces — the picker's accept is the submit lane, not a second one.
#[tokio::test]
async fn test_runtime_slash_round_trip_picker_accept_hands_off() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: `/` opens the inline picker; `n` narrows it to /new.
    driver
        .advance(&[key(TerminalKey::Char('/')), key(TerminalKey::Char('n'))])
        .await?;

    // -- Check: the picker is up and nothing has been handed off yet.
    assert!(
        driver.state().picker.active().is_some(),
        "typing / opens the inline slash picker"
    );
    assert!(
        driver.orchestrator_events().is_empty(),
        "narrowing the picker hands nothing off"
    );

    // -- Exec: Enter accepts the selected command.
    driver.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check: the accept closed the picker and handed off the command.
    assert!(
        driver.state().picker.active().is_none(),
        "accepting an inline slash closes the picker"
    );
    assert_eq!(
        submitted_texts(&driver),
        vec!["/new"],
        "the picker accept must hand off the same command the typed lane does"
    );
    assert!(
        driver.state().transcript.entries().is_empty(),
        "the picker accept appends no user echo row"
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    Ok(())
}

/// WHEN the user types `/`, narrows the inline picker to `/new`, and accepts it
/// with Tab, THE render loop SHALL hand off `/new` exactly once, close the
/// picker, push no echo row, and end idle — Tab and Enter share one accept.
#[tokio::test]
async fn test_runtime_slash_round_trip_picker_tab_accept_hands_off() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: `/` opens the inline picker; `n` narrows it to /new.
    driver
        .advance(&[key(TerminalKey::Char('/')), key(TerminalKey::Char('n'))])
        .await?;

    // -- Exec: Tab accepts the selected command.
    driver.advance(&[key(TerminalKey::Tab)]).await?;

    // -- Check: the accept closed the picker and handed off the command once.
    assert!(
        driver.state().picker.active().is_none(),
        "Tab closes the inline slash picker"
    );
    assert_eq!(
        submitted_texts(&driver),
        vec!["/new"],
        "Tab must hand the command off exactly once"
    );
    assert!(
        driver.state().transcript.entries().is_empty(),
        "the Tab accept appends no user echo row"
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    Ok(())
}

/// WHEN the user opens `/`, moves the highlight onto `/status`, narrows with `s`,
/// and presses Tab, THE render loop SHALL hand off `/status`: the accept reads
/// the highlighted row of the narrowed list, never a stale index.
#[tokio::test]
async fn test_runtime_slash_round_trip_picker_narrow_tab_accepts_highlighted() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: `/` opens the list; four Down presses highlight /status.
    driver
        .advance(&typed_keys(&[
            TerminalKey::Char('/'),
            TerminalKey::Down,
            TerminalKey::Down,
            TerminalKey::Down,
            TerminalKey::Down,
        ]))
        .await?;

    // -- Check: /status is the highlighted row before narrowing.
    assert_eq!(
        driver.state().picker.selected_slash_label().as_deref(),
        Some("/status"),
        "four Down presses highlight /status"
    );

    // -- Exec: `s` narrows the list; Tab accepts the highlight.
    driver
        .advance(&[key(TerminalKey::Char('s')), key(TerminalKey::Tab)])
        .await?;

    // -- Check: the narrowed list's first row is accepted.
    assert_eq!(
        submitted_texts(&driver),
        vec!["/status"],
        "the narrowing must reset the highlight to the first narrowed entry"
    );
    assert!(
        driver.state().picker.active().is_none(),
        "the accept closes the inline slash picker"
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    Ok(())
}

/// WHEN a Warning narration reaches the surface, THE surface SHALL show it on
/// the status lane and add no transcript row: the persisted row comes only from
/// the replay record, never from the live narration.
#[tokio::test]
async fn test_runtime_slash_round_trip_warning_narration_sets_status_only() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[DriveEvent::Narration(Box::new(system_message(
            NoticeKind::Warning,
            "careful now",
        )))])
        .await?;

    // -- Check: the status lane carries it; the transcript stays empty.
    assert_eq!(
        driver.state().status.message.status_line(),
        "careful now",
        "a warning is a transient status message"
    );
    assert!(
        driver.state().transcript.entries().is_empty(),
        "a live warning adds no transcript row"
    );
    Ok(())
}

/// WHEN a TurnError narration reaches the surface, THE surface SHALL add a
/// persisted notice row and close the turn to the idle phase.
#[tokio::test]
async fn test_runtime_slash_round_trip_turn_error_narration_persists_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[DriveEvent::Narration(Box::new(system_message(
            NoticeKind::TurnError,
            "the turn blew up",
        )))])
        .await?;

    // -- Check: one notice row carrying the error, and the surface is idle.
    let entries = driver.state().transcript.entries();
    assert_eq!(entries.len(), 1, "a turn error is one persisted row");
    let Some(entry) = entries.first() else {
        return Err("expected the turn-error notice row".into());
    };
    let TranscriptEntry::Notice(notice) = entry else {
        return Err("the turn error must land as a notice row".into());
    };
    assert!(
        notice.text.contains("the turn blew up"),
        "the row carries the error text; got: {}",
        notice.text
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    Ok(())
}

/// WHEN a resumed-session start record arrives, THE surface SHALL show no clear
/// and no banner: the history it resumed is already on screen.
#[tokio::test]
async fn test_runtime_slash_round_trip_resumed_start_keeps_the_surface() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.advance(&typed("hi")).await?;
    let before = driver.state().transcript.len();

    // -- Exec
    driver
        .advance(&[DriveEvent::Record(Box::new(session_started_resumed()))])
        .await?;

    // -- Check: untouched — no clear, no banner.
    assert_eq!(
        driver.state().transcript.len(),
        before,
        "a resumed start must not clear the rows it resumed"
    );
    assert!(
        !driver
            .state()
            .transcript
            .entries()
            .iter()
            .any(|entry| matches!(entry, TranscriptEntry::Banner(_))),
        "a resumed start pushes no banner"
    );
    Ok(())
}

// region:    --- Test Support

fn key(key: TerminalKey) -> DriveEvent {
    DriveEvent::Key(TerminalEvent::Key(key))
}

/// One drive event per key, in order.
fn typed_keys(keys: &[TerminalKey]) -> Vec<DriveEvent> {
    keys.iter().copied().map(key).collect()
}

/// Type `text` character by character, then submit it.
fn typed(text: &str) -> Vec<DriveEvent> {
    text.chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect()
}

/// The prompt texts the loop handed to the orchestrator channel.
fn submitted_texts(driver: &RenderLoopDriver) -> Vec<&str> {
    driver
        .orchestrator_events()
        .iter()
        .filter_map(|event| match event {
            OrchestratorEvent::PromptSubmitted { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support

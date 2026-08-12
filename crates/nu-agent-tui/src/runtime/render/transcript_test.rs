use nu_agent_core::transcript::items::{
    ProseMessage, Spacer, TranscriptEntry, TranscriptEntryKind,
};

use super::transcript::row_needs_user_bg;

fn user() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
}

fn assistant() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
}

fn spacer() -> TranscriptEntry {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Spacer(Spacer),
        status: None,
    }
}

#[test]
fn user_entry_needs_user_bg() {
    let entries = vec![user()];
    assert!(row_needs_user_bg(&entries, 0));
}

#[test]
fn assistant_entry_does_not_need_user_bg() {
    let entries = vec![assistant()];
    assert!(!row_needs_user_bg(&entries, 0));
}

#[test]
fn spacer_after_user_needs_user_bg() {
    let entries = vec![user(), spacer()];
    assert!(row_needs_user_bg(&entries, 1));
}

#[test]
fn spacer_before_user_needs_user_bg() {
    let entries = vec![spacer(), user()];
    assert!(row_needs_user_bg(&entries, 0));
}

#[test]
fn spacer_between_two_users_needs_user_bg() {
    let entries = vec![user(), spacer(), user()];
    assert!(row_needs_user_bg(&entries, 1));
}

#[test]
fn spacer_not_adjacent_to_user_does_not_need_user_bg() {
    let entries = vec![assistant(), spacer(), assistant()];
    assert!(!row_needs_user_bg(&entries, 1));
}

#[test]
fn out_of_range_entry_does_not_need_user_bg() {
    let entries = vec![user()];
    assert!(!row_needs_user_bg(&entries, 5));
}

#[test]
fn non_separator_non_user_entries_do_not_need_user_bg() {
    let entries = vec![assistant()];
    assert!(!row_needs_user_bg(&entries, 0));
}

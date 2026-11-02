//! The compaction call line's life: one `Task` entry from start to finish, its
//! status mutated in place, and a replay that never spins.

use super::*;
use agnt_core::transcript::ir::NoticeKind;
use agnt_core::transcript::renderer::ItemStatus;

/// The transcript's compaction task lines, in order.
fn task_lines(state: &AppState) -> Vec<(String, String, ItemStatus)> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Task(task) => {
                Some((task.name.clone(), task.summary.clone(), task.status))
            }
            _ => None,
        })
        .collect()
}

/// WHEN a compaction starts, THE transcript SHALL hold ONE task line, in
/// progress, on the source it works on.
#[test]
fn compaction_start_pushes_one_in_progress_task_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_compaction(&mut state, started("auto"));

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 1, "one line for the whole life: {lines:?}");
    assert_eq!(lines[0].0, "Compaction");
    assert_eq!(lines[0].1, "auto");
    assert_eq!(lines[0].2, ItemStatus::InProgress);
    assert_eq!(
        state.transcript.entries().len(),
        1,
        "no extra lifecycle row"
    );
    Ok(())
}

/// WHEN the compaction completes, THE same line SHALL turn `Done` in place: no
/// second row and no rewritten text.
#[test]
fn compaction_complete_mutates_the_same_line_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("auto"));
    let before = task_lines(&state);

    // -- Exec
    reduce_compaction(
        &mut state,
        completed("auto", "preview", "## Summary\n- one"),
    );

    // -- Check
    let after = task_lines(&state);
    assert_eq!(after.len(), 1, "the line is never re-pushed: {after:?}");
    assert_eq!(after[0].0, before[0].0, "the name is unchanged");
    assert_eq!(after[0].1, before[0].1, "the summary is unchanged");
    assert_eq!(after[0].2, ItemStatus::Done);
    // The completed block holds the line and its summary, nothing else.
    assert_eq!(
        state
            .transcript
            .entries()
            .iter()
            .filter(|entry| matches!(entry, TranscriptEntry::Task(_)))
            .count(),
        1,
        "no additional lifecycle row appears"
    );
    Ok(())
}

/// WHEN the compaction fails, THE same line SHALL turn `Failed` and the system
/// notice SHALL report the failure.
#[test]
fn compaction_failure_marks_the_same_line_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("auto"));

    // -- Exec
    reduce_compaction(
        &mut state,
        SystemEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "summarizer timed out".to_string(),
        },
    );

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 1, "one line, status only: {lines:?}");
    assert_eq!(lines[0].2, ItemStatus::Failed);
    let notices = state
        .transcript
        .entries()
        .iter()
        .filter(|entry| {
            matches!(entry, TranscriptEntry::Notice(notice) if notice.kind == NoticeKind::System)
        })
        .count();
    assert_eq!(notices, 1, "the failure still reads as a system notice");
    Ok(())
}

/// WHEN cap eviction lands between start and finish, THE finish SHALL still
/// find the line: the lookup scans, it never counts indices.
#[test]
fn compaction_finish_finds_the_line_after_cap_eviction() -> Result<()> {
    // -- Setup & Fixtures: the line sits behind the cap's oldest entries
    let mut state = AppState::default();
    for index in 0..1_990 {
        state.transcript.push_entry(TranscriptEntry::Message(
            agnt_core::transcript::entry::MessageEntry {
                role: MessageRole::Assistant,
                markdown: format!("line {index}"),
                timestamp_millis: None,
                model: None,
                agent: None,
            },
        ));
    }
    reduce_compaction(&mut state, started("auto"));
    // Overflow the cap: the head entries are evicted, the line shifts with them.
    for index in 0..40 {
        state.transcript.push_entry(TranscriptEntry::Message(
            agnt_core::transcript::entry::MessageEntry {
                role: MessageRole::Assistant,
                markdown: format!("tail {index}"),
                timestamp_millis: None,
                model: None,
                agent: None,
            },
        ));
    }
    assert!(
        state.transcript.evicted_total() > 0,
        "the cap must have evicted entries between start and finish"
    );

    // -- Exec
    reduce_compaction(&mut state, completed("auto", "preview", "body"));

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 1, "the line survived the eviction: {lines:?}");
    assert_eq!(lines[0].2, ItemStatus::Done, "the finish reached it");
    Ok(())
}

/// WHEN `/compact` runs twice on the same source, THE second cycle SHALL open
/// its own line and reach `Done`: a terminal line never blocks a new start.
#[test]
fn repeated_slash_compaction_opens_a_fresh_line_per_cycle() -> Result<()> {
    // -- Setup & Fixtures: one completed cycle.
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("slash"));
    reduce_compaction(
        &mut state,
        completed("slash", "preview 1", "## Summary\n- one"),
    );

    // -- Exec: the second cycle.
    reduce_compaction(&mut state, started("slash"));

    // -- Check: the new cycle has its own running line.
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 2, "each cycle opens its own line: {lines:?}");
    assert_eq!(lines[0].2, ItemStatus::Done);
    assert_eq!(lines[1].2, ItemStatus::InProgress);

    // -- Exec & Check: the second completion marks the second line.
    reduce_compaction(
        &mut state,
        completed("slash", "preview 2", "## Summary\n- two"),
    );
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 2, "no line is re-pushed: {lines:?}");
    assert_eq!(lines[0].2, ItemStatus::Done);
    assert_eq!(lines[1].2, ItemStatus::Done, "the second line is marked");
    Ok(())
}

/// WHEN an auto compaction fails and the next turn retriggers it, THE retrigger
/// SHALL open a fresh line rather than reuse the failed one.
#[test]
fn failed_auto_compaction_retrigger_opens_a_fresh_line() -> Result<()> {
    // -- Setup & Fixtures: an auto compaction that failed.
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("auto"));
    reduce_compaction(
        &mut state,
        SystemEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "summarizer timed out".to_string(),
        },
    );

    // -- Exec: the next turn retries automatically.
    reduce_compaction(&mut state, started("auto"));

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 2, "the retry runs on its own line: {lines:?}");
    assert_eq!(lines[0].2, ItemStatus::Failed);
    assert_eq!(lines[1].2, ItemStatus::InProgress);
    Ok(())
}

/// WHEN a session replays two compactions, EACH SHALL push its own `Done` line:
/// the replay answers one line per completion, not one per source.
#[test]
fn double_history_replay_pushes_one_done_line_per_completion() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_compaction(
        &mut state,
        completed("history", "preview 1", "## Summary\n- one"),
    );
    reduce_compaction(
        &mut state,
        completed("history", "preview 2", "## Summary\n- two"),
    );

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(
        lines.len(),
        2,
        "one line per replayed completion: {lines:?}"
    );
    assert!(
        lines.iter().all(|line| line.2 == ItemStatus::Done),
        "each line arrives Done directly: {lines:?}"
    );
    Ok(())
}

/// WHEN a session replays a completed compaction, THE line SHALL arrive `Done`
/// directly: the session never showed a spinner, so none is invented.
#[test]
fn replayed_completion_pushes_a_done_line_directly() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec: the history replay folds the completion alone
    reduce_compaction(
        &mut state,
        completed("history", "preview", "## Summary\n- one"),
    );

    // -- Check
    let lines = task_lines(&state);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].2, ItemStatus::Done, "no spinner flash: {lines:?}");
    assert!(!state.compaction.in_progress(), "nothing is left running");
    Ok(())
}

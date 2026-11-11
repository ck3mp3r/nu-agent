//! Unit tests for the loop's buffered-completion drain.

use super::*;

use crate::orchestrator::external::ExternalCompletion;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN a user cancel arrives while one or more completions wait, THE
/// `CompletionBuffer::drain` method SHALL return the entries in arrival order
/// and leave `take_next` at None thereafter.
#[test]
fn test_pending_completions_drain_returns_the_entries_in_arrival_order() -> Result<()> {
    // -- Setup & Fixtures
    let mut buffer = CompletionBuffer::default();
    buffer.push(ExternalCompletion {
        task_id: "task-a".to_string(),
        prompt: "prompt a".to_string(),
    });
    buffer.push(ExternalCompletion {
        task_id: "task-b".to_string(),
        prompt: "prompt b".to_string(),
    });

    // -- Exec
    let drained = buffer.drain();

    // -- Check
    let prompts: Vec<&str> = drained.iter().map(|entry| entry.prompt.as_str()).collect();
    assert_eq!(
        prompts,
        vec!["prompt a", "prompt b"],
        "the user cancel must get the buffered completions back in arrival order"
    );
    assert!(
        buffer.take_next().is_none(),
        "the drained buffer must hold no completion to dispatch"
    );
    Ok(())
}

/// WHEN the buffer holds no completion at cancel time, THE
/// `CompletionBuffer::drain` method SHALL return no entries and keep the
/// buffer empty.
#[test]
fn test_pending_completions_drain_on_an_empty_buffer_returns_no_entries() -> Result<()> {
    // -- Setup & Fixtures
    let mut buffer = CompletionBuffer::default();

    // -- Exec
    let drained = buffer.drain();

    // -- Check
    assert!(
        drained.is_empty(),
        "draining an empty buffer must return no entries"
    );
    assert!(
        buffer.take_next().is_none(),
        "an empty buffer holds no completion to dispatch"
    );
    Ok(())
}

/// WHEN a fresh completion arrives after such a cancel, THE
/// `CompletionBuffer` SHALL keep its normal intake: the arrival waits in the
/// buffer and dispatches in its own turn.
#[test]
fn test_pending_completions_arrival_after_a_drain_keeps_its_normal_dispatch() -> Result<()> {
    // -- Setup & Fixtures
    let mut buffer = CompletionBuffer::default();
    buffer.push(ExternalCompletion {
        task_id: "task-a".to_string(),
        prompt: "prompt a".to_string(),
    });
    buffer.drain();

    // -- Exec
    buffer.push(ExternalCompletion {
        task_id: "task-b".to_string(),
        prompt: "prompt b".to_string(),
    });

    // -- Check
    assert!(
        !buffer.is_empty(),
        "a completion that arrives after the drain must wait in the buffer again"
    );
    let next = buffer
        .take_next()
        .ok_or("the post-drain arrival should be buffered")?;
    assert_eq!(
        next.prompt, "prompt b",
        "the post-drain arrival must dispatch in its own turn"
    );
    Ok(())
}

//! Unit tests for the loop's external intake queue.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_external_queue_cancel_in_flight_stops_the_running_turn() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let entry = queue.take_next().ok_or("task-a should be queued")?;
    queue.begin_turn(entry.task_id);

    // -- Exec
    let disposition = queue.cancel("task-a".to_string());

    // -- Check
    assert_eq!(disposition, CancelDisposition::StopsInFlight);
    Ok(())
}

#[test]
fn test_external_queue_cancel_queued_waits_for_the_dispatch() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());
    queue.admit("task-b".to_string(), "prompt b".to_string());
    let running = queue.take_next().ok_or("task-a should be queued")?;
    let a_cancels_at_entry = queue.begin_turn(running.task_id);

    // -- Exec
    let disposition = queue.cancel("task-b".to_string());
    queue.finish_turn();
    let next = queue.take_next().ok_or("task-b should be queued")?;
    let b_cancels_at_entry = queue.begin_turn(next.task_id);

    // -- Check
    assert!(
        !a_cancels_at_entry,
        "no cancel named task-a, so its turn is not cancelled at entry"
    );
    assert_eq!(disposition, CancelDisposition::WaitsForDispatch);
    assert!(
        b_cancels_at_entry,
        "the cancel that waited for task-b must stop its turn at entry"
    );
    Ok(())
}

#[test]
fn test_external_queue_cancel_for_an_unarrived_task_waits_for_its_arrival() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();

    // -- Exec
    let disposition = queue.cancel("task-a".to_string());
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let entry = queue.take_next().ok_or("task-a should be queued now")?;
    let cancels_at_entry = queue.begin_turn(entry.task_id);

    // -- Check
    assert_eq!(disposition, CancelDisposition::WaitsForDispatch);
    assert!(
        cancels_at_entry,
        "a cancel for a task that had not arrived must be honoured at its arrival"
    );
    Ok(())
}

#[test]
fn test_external_queue_repeated_cancel_is_honoured_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let first = queue.take_next().ok_or("task-a should be queued")?;
    queue.begin_turn(first.task_id);
    queue.finish_turn();

    // -- Exec
    let first_disposition = queue.cancel("task-a".to_string());
    let repeat_disposition = queue.cancel("task-a".to_string());
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let dispatched = queue.take_next().ok_or("task-a should be queued again")?;
    let cancels_at_entry = queue.begin_turn(dispatched.task_id);
    queue.finish_turn();

    // The repeat cancel is spent: the next turn for the same task runs.
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let next = queue
        .take_next()
        .ok_or("task-a should be queued a third time")?;
    let later_cancels_at_entry = queue.begin_turn(next.task_id);

    // -- Check
    assert_eq!(first_disposition, CancelDisposition::WaitsForDispatch);
    assert_eq!(repeat_disposition, CancelDisposition::WaitsForDispatch);
    assert!(
        cancels_at_entry,
        "the repeated cancel must stop the turn it named"
    );
    assert!(
        !later_cancels_at_entry,
        "a cancel is honoured exactly once: the later turn of the same task runs"
    );
    Ok(())
}

#[test]
fn test_external_queue_begin_turn_without_a_waiting_cancel_is_not_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());

    // -- Exec
    let entry = queue.take_next().ok_or("task-a should be queued")?;
    let cancels_at_entry = queue.begin_turn(entry.task_id);

    // -- Check
    assert!(
        !cancels_at_entry,
        "no cancel was queued, so the dispatch must request none"
    );
    Ok(())
}

#[test]
fn test_external_queue_drain_returns_the_queued_prompt_and_clears_the_waiting_cancel() -> Result<()>
{
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let waiting = queue.cancel("task-a".to_string());

    // -- Exec
    let drained = queue.drain();

    // -- Check
    assert_eq!(waiting, CancelDisposition::WaitsForDispatch);
    assert_eq!(
        drained,
        vec![ExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        }],
        "the user cancel must get the queued prompt back"
    );
    assert!(
        queue.take_next().is_none(),
        "the drained queue must hold no prompt to dispatch"
    );
    queue.admit("task-a".to_string(), "prompt a".to_string());
    let entry = queue.take_next().ok_or("task-a should be queued again")?;
    let cancels_at_entry = queue.begin_turn(entry.task_id);
    assert!(
        !cancels_at_entry,
        "the drained waiting cancel must not fire a phantom cancel at the repeat arrival's dispatch"
    );
    Ok(())
}

#[test]
fn test_external_queue_drain_returns_prompts_in_arrival_order() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();
    queue.admit("task-a".to_string(), "prompt a".to_string());
    queue.admit("task-b".to_string(), "prompt b".to_string());

    // -- Exec
    let drained = queue.drain();

    // -- Check
    let ids: Vec<&str> = drained.iter().map(|entry| entry.task_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["task-a", "task-b"],
        "the drain must return the prompts in arrival order"
    );
    assert!(
        queue.take_next().is_none(),
        "the drained queue must be empty"
    );
    Ok(())
}

#[test]
fn test_external_queue_drain_on_an_empty_queue_returns_no_entries() -> Result<()> {
    // -- Setup & Fixtures
    let mut queue = ExternalQueue::default();

    // -- Exec
    let drained = queue.drain();

    // -- Check
    assert!(
        drained.is_empty(),
        "draining an empty queue must return no entries"
    );
    assert!(
        queue.take_next().is_none(),
        "an empty queue holds no prompt to dispatch"
    );
    Ok(())
}
